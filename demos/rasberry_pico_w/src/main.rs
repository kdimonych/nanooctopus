#![doc = include_str!("../README.md")]
#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

// Logging and panicking behavior can be customized by implementing the `defmt::Logger`
// and `core::panic::PanicInfo` traits, respectively.
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_rp::{
    bind_interrupts,
    clocks::RoscRng,
    dma::Channel as DmaChannel,
    dma::InterruptHandler as DmaInterruptHandler,
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIO0},
    pio::{InterruptHandler as PioInterruptHandler, Pio},
};

use cyw43::JoinAuth;
use cyw43::JoinOptions;
use cyw43::NetDriver;
use cyw43_firmware::{CYW43_43439A0, CYW43_43439A0_CLM, NVRAM_RP2040};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};

use core::fmt::{Debug, Display};
use core::str::FromStr;
use heapless::String;
use static_cell::StaticCell;

use edge_nal::TcpBind;
use edge_nal_embassy::{Tcp, TcpBuffers};
use nanooctopus_server::socket::*;
use nanooctopus_server::*;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
    DMA_IRQ_0 => DmaInterruptHandler<DMA_CH0>;
});

// Get version from Cargo.toml at compile time
const VERSION: &str = env!("CARGO_PKG_VERSION");

const NETWORK_STACK_SOCKETS: usize = 20;

static NETWORK_RESOURCES: StaticCell<StackResources<NETWORK_STACK_SOCKETS>> = StaticCell::new();
static CY43_STATE: StaticCell<cyw43::State> = StaticCell::new();

const SOCKETS: usize = 8; // Number of simultaneous sockets the server can accept and handle; adjust as needed
const RX_SIZE: usize = 256; // Size of the receive buffer for each socket; adjust as needed
const TX_SIZE: usize = 256; // Size of the transmit buffer for each socket; adjust as needed

type PioSpi0 = PioSpi<'static, PIO0, 0>;
type SpiBus0 = cyw43::SpiBus<Output<'static>, PioSpi0>;
type Cy43Runner = cyw43::Runner<'static, SpiBus0>;
type NetStackRunner = embassy_net::Runner<'static, NetDriver<'static>>;

/// Just returns a mask string. Nothing fancy, but it prevents the password from being accidentally logged in plaintext.
const fn mask_password(_: &str) -> &'static str {
    "******"
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    /**************************************************************************************************/
    /*                                Initialize the network stack                                    */
    /**************************************************************************************************/

    // These environment variables are forwarded from the build script, which reads them from the .env file.
    // This allows us to keep the WiFi credentials out of the source code and instead manage them through environment variables.
    // Create the .env file in the project root with the following content:
    // WIFI_SSID=your_wifi_ssid
    // WIFI_PASSWORD=your_wifi_password
    let ssid: String<32> = heapless::String::from_str(option_env!("WIFI_SSID").unwrap_or("None")).unwrap();
    let password = heapless::String::<64>::from_str(option_env!("WIFI_PASSWORD").unwrap_or("")).unwrap();

    // Initialize peripherals
    let p: embassy_rp::Peripherals = embassy_rp::init(Default::default());

    let pwr: Output<'_> = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let dma = p.DMA_CH0;

    let spi: PioSpi0 = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        DmaChannel::new(dma, Irqs),
    );

    // Firmware binary included in the cyw43_firmware crate;
    let fw = CYW43_43439A0;
    let nvram = NVRAM_RP2040;

    defmt::info!("Creating WiFi driver...");
    let cyw43_state = CY43_STATE.init(cyw43::State::new());

    let (wifi_network_driver, mut control, cyw43_runner) = cyw43::new(cyw43_state, pwr, spi, fw, nvram).await;
    defmt::info!("WiFi driver created.");

    // Spawn the CYW43 runner task. Spawning this task here guarantees the WiFi driver operates correctly.
    spawner.spawn(wifi_runner_task(cyw43_runner).unwrap());

    // Initialize the WiFi hardware with CLM data
    defmt::debug!("Initializing WiFi driver...");
    let clm = CYW43_43439A0_CLM; // CLM binary included in the cyw43_firmware crate;
    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::Performance)
        .await;
    defmt::info!("WiFi driver initialized.");

    defmt::info!("Attempting to join SSID: {}", ssid.as_str());
    defmt::info!("Attempting to join with password: {}", mask_password(password.as_str()));

    let mut join_options = JoinOptions::default();
    join_options.auth = JoinAuth::Wpa2Wpa3;
    join_options.passphrase = password.as_str().as_bytes();

    while let Err(e) = control.join(ssid.as_str(), join_options.clone()).await {
        defmt::error!(
            "Failed to join WiFi network: {:?}. Retrying...",
            defmt::Debug2Format(&e)
        );
    }
    defmt::info!("Successfully joined WiFi network");

    defmt::info!("Configuring network stack...");
    let stack_resources = NETWORK_RESOURCES.init(StackResources::new());

    defmt::info!("configuring network stack with DHCP");
    let mut rng = RoscRng;
    let seed = rng.next_u64();
    let (net_stack, runner) = embassy_net::new(
        wifi_network_driver,
        embassy_net::Config::dhcpv4(Default::default()),
        stack_resources,
        seed,
    );
    spawner.spawn(wifi_network_runner(runner).unwrap());

    net_stack.wait_link_up().await;
    defmt::info!("Network link is up.");
    net_stack.wait_config_up().await;
    defmt::info!("Network configuration is up.");

    let config = net_stack.config_v4().unwrap_or_else(|| {
        defmt::panic!("Failed to get network configuration.");
    });

    defmt::info!("IPv4 address: {}", config.address);
    defmt::info!("IPv4 gateway: {}", config.gateway);
    defmt::info!("IPv4 DNS servers: {:?}", config.dns_servers);

    /**************************************************************************************************/
    /*                                    Initialize the server                                       */
    /**************************************************************************************************/

    // Create a local endpoint for the server to listen on (e.g., port 8080)
    // For embassy-net, the IP address is not used as it is determined by the network stack
    // configuration, but we still need to provide a valid SocketAddr.
    // We can use the wildcard address (0.0.0.0) to listen on all available interfaces or
    // just use the assigned IP for consistency.

    let buffers = TcpBuffers::<SOCKETS, RX_SIZE, TX_SIZE>::new();
    let tcp = Tcp::new(net_stack, &buffers);
    let acceptor = tcp.bind("0.0.0.0:8080".parse().unwrap()).await.unwrap();

    let mut server = DefaultServer::new();
    let srv_config = Config {
        keepalive_timeout_ms: None,
    };

    let h = map_handler!(
        ("/", root: HtmlHandler<'static> = HtmlHandler::new(include_str!("../index.html"))),
        ("/version", version: PlainTextHandler<'static> = PlainTextHandler::new(VERSION)),
        ("/hello_world", hw: RootHandler = RootHandler {}),
        ("/favicon.ico", fav: FaviconHandler<'static> = FaviconHandler::new(include_bytes!("../favicon.ico")))
    );

    defmt::info!("\n\nHTTP server is running and ready to accept requests.");
    defmt::info!("Visit http://{}:8080/", config.address.address());

    defmt::info!("To check the number of active connections the server can handle, run the script from project root:");
    defmt::info!(
        "./scripts/hold_open_load.py --single-shot-connection -c {} --host {} --port 8080 --path /hello_world \n\n",
        SOCKETS,
        config.address.address(),
    );

    server.run::<_, _, SOCKETS>(srv_config, acceptor, h).await.unwrap();
    unreachable!();
}

#[embassy_executor::task]
async fn wifi_runner_task(runner: Cy43Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_network_runner(mut net_runner: NetStackRunner) -> ! {
    net_runner.run().await
}

struct RootHandler;
impl Handler for RootHandler {
    type Error<E>
        = IoError<E>
    where
        E: Debug;

    async fn handle<S, const CN: usize>(
        &self,
        task_id: impl Display + Copy,
        conn: &mut Connection<'_, S, CN>,
    ) -> Result<(), Self::Error<S::Error>>
    where
        S: SocketRead + SocketWrite + SocketSplit,
    {
        defmt::info!(
            "Hello World task {}: Handling request for root path '/'",
            defmt::Display2Format(&task_id)
        );
        conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
            .await?;
        conn.write_all(b"Raspberry Pico W: Hello World!").await?;
        conn.flush().await?;
        conn.complete().await?;
        defmt::info!(
            "Hello World task {}: Completed request for root path '/'",
            defmt::Display2Format(&task_id)
        );

        Ok(())
    }
}
