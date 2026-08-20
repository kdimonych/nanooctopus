#![doc = include_str!("../README.md")]
#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

// Logging and panicking behavior can be customized by implementing the `defmt::Logger`
// and `core::panic::PanicInfo` traits, respectively.
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_net::{Stack, StackResources};
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

use core::fmt::Write;
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
const CONNECTION_KEEPALIVE_TIMEOUT_MS: Option<u32> = Some(1000); // 1000 millisecond keepalive timeout; adjust as needed

const SOCKETS: usize = 4; // Number of simultaneous sockets the server can accept and handle; adjust as needed
const RX_SIZE: usize = 256; // Size of the receive buffer for each socket; adjust as needed
const TX_SIZE: usize = 256; // Size of the transmit buffer for each socket; adjust as needed
const HANDLER_BUFFER: usize = 1024; //The buffer that  might be used by request handlers to process data.

type PioSpi0 = PioSpi<'static, PIO0, 0>;
type SpiBus0 = cyw43::SpiBus<Output<'static>, PioSpi0>;
type Cy43Runner = cyw43::Runner<'static, SpiBus0>;
type NetStackRunner = embassy_net::Runner<'static, NetDriver<'static>>;

static NETWORK_RESOURCES: StaticCell<StackResources<NETWORK_STACK_SOCKETS>> = StaticCell::new();
static CY43_STATE: StaticCell<cyw43::State> = StaticCell::new();
static TCP_BUFFERS: StaticCell<TcpBuffers<SOCKETS, RX_SIZE, TX_SIZE>> = StaticCell::new();
static HTTP_SERVER: StaticCell<DefaultServer> = StaticCell::new();

/// Just returns a mask string. Nothing fancy, but it prevents the password from being accidentally logged in plaintext.
const fn mask_password(_: &str) -> &'static str {
    "******"
}

use core::mem::MaybeUninit;
use cortex_m::asm;
use cortex_m_rt::{ExceptionFrame, exception};

static mut LAST_HARDFAULT: MaybeUninit<ExceptionFrame> = MaybeUninit::uninit();

#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    unsafe {
        core::ptr::write(core::ptr::addr_of_mut!(LAST_HARDFAULT) as *mut ExceptionFrame, *ef);
    }
    defmt::error!("HardFault: {:?}", defmt::Debug2Format(ef));

    loop {
        asm::bkpt();
    }
}

#[inline(always)]
fn debug_memory_layout(name: &'static str) {
    unsafe extern "C" {
        static _ram_start: u32;
        unsafe static _ram_end: u32;
        static _stack_start: u32;
        static _stack_end: u32;
        static _stack_size: u32;
    }

    let ram_start = { &raw const _ram_start as usize };
    let ram_end = { &raw const _ram_end as usize };
    let stack_start = { &raw const _stack_start as usize };
    let stack_end = { &raw const _stack_end as usize };
    let msp = cortex_m::register::msp::read() as usize;
    let psp = cortex_m::register::psp::read() as usize;
    let stack_size = stack_start - stack_end;
    let stack_usage = stack_start - msp;

    log::info!("=== Memory Layout: {} ===", name);
    unsafe {
        let sp = core::ptr::read_volatile(0x10000100 as *const u32);
        let reset = core::ptr::read_volatile(0x10000104 as *const u32);

        defmt::info!("Vector sp = 0x{:08x}", sp);
        defmt::info!("Reset vec = 0x{:08x}", reset);
    }

    log::info!("RAM Start:    0x{:08x}", ram_start);
    log::info!("RAM End:      0x{:08x}", ram_end);
    log::info!("Stack Start:  0x{:08x}", stack_start);
    log::info!("Stack End:    0x{:08x}", stack_end);
    log::info!("Stack Size:   0x{:08x} ({}) bytes", stack_size, stack_size);
    log::info!("Stack Usage:   0x{:08x} ({}) bytes", stack_usage, stack_usage);
    log::info!("Current MSP:   0x{:08x}", msp);
    log::info!("Current PSP:   0x{:08x}", psp);
}

static EXECUTOR0: StaticCell<embassy_executor::Executor> = StaticCell::new();
#[cortex_m_rt::entry]
fn main() -> ! {
    let executor0 = EXECUTOR0.init(embassy_executor::Executor::new());
    executor0.run(move |spawner| {
        spawner.spawn(core_0_task(spawner).unwrap());
    });
}

#[embassy_executor::task]
async fn core_0_task(spawner: Spawner) {
    debug_memory_layout("main");
    embassy_rp::install_core0_stack_guard().unwrap();

    /**************************************************************************************************/
    /*                                Initialize the network stack                                    */
    /**************************************************************************************************/
    let wifi_network_driver = init_wifi_network_driver(spawner).await;
    let (net_stack, ip_address) = init_network(spawner, wifi_network_driver).await;

    /**************************************************************************************************/
    /*                                    Start the server                                       */
    /**************************************************************************************************/
    spawner.spawn(run_server_task(net_stack, ip_address).unwrap());
}

#[embassy_executor::task]
async fn run_server_task(net_stack: Stack<'static>, ip_address: embassy_net::Ipv4Address) -> ! {
    debug_memory_layout("run_server");
    // Create a local endpoint for the server to listen on (e.g., port 8080)
    // For embassy-net, the IP address is not used as it is determined by the network stack
    // configuration, but we still need to provide a valid SocketAddr.
    // We can use the wildcard address (0.0.0.0) to listen on all available interfaces or
    // just use the assigned IP for consistency.
    let buffers = TCP_BUFFERS.init_with(TcpBuffers::new);
    let tcp = Tcp::new(net_stack, buffers);
    let acceptor = tcp.bind("0.0.0.0:8080".parse().unwrap()).await.unwrap();

    let srv_config = Config {
        keepalive_timeout_ms: CONNECTION_KEEPALIVE_TIMEOUT_MS,
    };

    defmt::info!("\n\nHTTP server is running and ready to accept requests.");
    defmt::info!("Visit http://{}:8080/", ip_address);

    defmt::info!("To check the number of active connections the server can handle, run the script from project root:");
    if CONNECTION_KEEPALIVE_TIMEOUT_MS.is_some() {
        defmt::info!(
            "  ./scripts/hold_open_load.py -c {} --host {} --port 8080 --path /hello_world",
            SOCKETS,
            ip_address
        );
    } else {
        defmt::info!(
            "  ./scripts/hold_open_load.py --single-shot-connection -c {} --host {} --port 8080 --path /hello_world",
            SOCKETS,
            ip_address
        );
    }

    let h = map_handler!(HANDLER_BUFFER, (),
        ("/", root: HtmlHandler<'static> = HtmlHandler::new(include_str!("../index.html"))),
        ("/version", version: PlainTextHandler<'static> = PlainTextHandler::new(VERSION)),
        ("/hello_world", hw: HelloWorldHandler = HelloWorldHandler {}),
        ("/favicon.ico", fav: FaviconHandler<'static> = FaviconHandler::new(include_bytes!("../favicon.ico")))
    );

    let server = HTTP_SERVER.init_with(DefaultServer::new);
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

struct HelloWorldHandler;
impl EndpointHandler for HelloWorldHandler {
    type Error<E>
        = IoError<E>
    where
        E: Debug;

    fn supported_methods() -> &'static [Method] {
        &[Method::Get]
    }

    async fn handle<S, const CN: usize>(
        &self,
        _ctx: impl Copy,
        task_id: impl Display + Copy,
        conn: &mut Connection<'_, S, CN>,
        _allocator: PrefixArena<'_>,
    ) -> Result<(), Self::Error<S::Error>>
    where
        S: SocketRead + SocketWrite + SocketSplit,
    {
        const RESPONSE_BODY: &[u8] = b"Raspberry Pico W: Hello World!";
        let mut content_length_str = heapless::String::<16>::new();
        write!(&mut content_length_str, "{}", RESPONSE_BODY.len()).map_err(|_| IoError::InvalidState)?;

        defmt::info!(
            "Hello World task {}: Handling request for root path '/hello_world'",
            defmt::Display2Format(&task_id)
        );
        conn.initiate_response(
            200,
            Some("OK"),
            &[
                (H_CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN),
                (H_CONTENT_LENGTH, content_length_str.as_str()),
            ],
        )
        .await?;
        conn.write_all(RESPONSE_BODY).await?;
        conn.flush().await?;
        conn.complete().await?;
        defmt::info!(
            "Hello World task {}: Completed request for root path '/hello_world'",
            defmt::Display2Format(&task_id)
        );

        Ok(())
    }
}

#[inline(never)]
async fn init_wifi_network_driver(spawner: Spawner) -> NetDriver<'static> {
    debug_memory_layout("init_wifi_network_driver");
    // Initialize peripherals
    let p: embassy_rp::Peripherals = embassy_rp::init(Default::default());

    let pwr = Output::new(p.PIN_23, Level::Low);
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
    debug_memory_layout("CY43_STATE.init_with");
    let cyw43_state = CY43_STATE.init_with(cyw43::State::new);

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

    // These environment variables are forwarded from the build script, which reads them from the .env file.
    // This allows us to keep the WiFi credentials out of the source code and instead manage them through environment variables.
    // Create the .env file in the project root with the following content:
    // WIFI_SSID=your_wifi_ssid
    // WIFI_PASSWORD=your_wifi_password
    let ssid: String<32> = heapless::String::from_str(option_env!("WIFI_SSID").unwrap_or("None")).unwrap();
    let password = heapless::String::<64>::from_str(option_env!("WIFI_PASSWORD").unwrap_or("")).unwrap();

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

    wifi_network_driver
}

#[inline(never)]
async fn init_network(
    spawner: Spawner,
    wifi_network_driver: NetDriver<'static>,
) -> (Stack<'static>, embassy_net::Ipv4Address) {
    debug_memory_layout("init_network");
    defmt::info!("Configuring network stack...");
    let stack_resources = NETWORK_RESOURCES.init_with(StackResources::new);

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

    (net_stack, config.address.address())
}
