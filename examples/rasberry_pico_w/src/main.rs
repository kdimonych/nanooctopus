#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

use core::str::FromStr;

// Logging and panicking behavior can be customized by implementing the `defmt::Logger`
// and `core::panic::PanicInfo` traits, respectively.
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_rp::{
    bind_interrupts,
    clocks::RoscRng,
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIO0},
    pio::{InterruptHandler as PioInterruptHandler, Pio},
};
use static_cell::StaticCell;

use cyw43::NetDriver;
use cyw43_firmware::{CYW43_43439A0, CYW43_43439A0_CLM};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};

use cyw43::JoinAuth;
use cyw43::JoinOptions;

use heapless::String;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
});

const NETWORK_STACK_SOCKETS: usize = 20;

static NETWORK_RESOURCES: StaticCell<StackResources<NETWORK_STACK_SOCKETS>> = StaticCell::new();
static CY43_STATE: StaticCell<cyw43::State> = StaticCell::new();

type PioSpi0 = PioSpi<'static, PIO0, 0, DMA_CH0>;
type Cy43Runner = cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>;
type NetStackRunner = embassy_net::Runner<'static, NetDriver<'static>>;

/// Just returns a mask string. Nothing fancy, but it prevents the password from being accidentally logged in plaintext.
const fn mask_password(_: &str) -> &'static str {
    "******"
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
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
        dma,
    );

    // Firmware binary included in the cyw43_firmware crate;
    let fw = CYW43_43439A0;

    defmt::info!("Creating WiFi driver...");
    let cyw43_state = CY43_STATE.init(cyw43::State::new());
    let (wifi_network_driver, mut control, cyw43_runner) = cyw43::new(cyw43_state, pwr, spi, fw).await;
    defmt::info!("WiFi driver created.");

    // Spawn the CYW43 runner task. Spawning this task here guarantees the WiFi driver operates correctly.
    spawner.spawn(wifi_runner_task(cyw43_runner)).unwrap();

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
    spawner.spawn(wifi_network_runner(runner)).unwrap();

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

    loop {
        embassy_futures::yield_now().await;
    }
}

#[embassy_executor::task]
async fn wifi_runner_task(runner: Cy43Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_network_runner(mut net_runner: NetStackRunner) -> ! {
    net_runner.run().await
}
