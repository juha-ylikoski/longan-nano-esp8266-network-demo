#![no_std]
#![no_main]

use core::{
    borrow::Borrow,
    cell::RefCell,
    fmt::Write,
    ops::DerefMut,
    ptr::NonNull,
    str::{from_utf8, FromStr},
};

use embedded_graphics::prelude::*;
use embedded_graphics::{
    mono_font::{jis_x0201::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use heapless::{String, Vec};
use longan_nano::{
    hal::prelude::*,
    led::{rgb, Led},
};
use longan_nano::{
    hal::{
        delay::McycleDelay,
        eclic::EclicExt,
        gpio::GpioExt,
        pac::{Interrupt, Peripherals, ECLIC, USART1},
        rcu::RcuExt,
        serial::{Config, Error, Rx, Serial},
    },
    lcd, lcd_pins,
};
use nb;
use panic_halt as _;
use riscv::interrupt::{self, CriticalSection};
use riscv_rt::entry;

macro_rules! UART_CONFIG {
    () => {
        Config {
            baudrate: 115200.bps(),
            parity: longan_nano::hal::serial::Parity::ParityNone,
            stopbits: longan_nano::hal::serial::StopBits::STOP1,
        }
    };
}

static mut _UART_BUFFER: interrupt::Mutex<heapless::spsc::Queue<u8, 128>> =
    interrupt::Mutex::new(heapless::spsc::Queue::<u8, 128>::new());
static mut _USART1_RX: interrupt::Mutex<RefCell<Option<Rx<USART1>>>> =
    interrupt::Mutex::new(RefCell::new(None));
// #[export_name = "USART1"]
fn uart_interrupt() {
    interrupt::free(|_| unsafe {
        let rx = _USART1_RX.get_mut().get_mut().as_mut().unwrap();
        let data = rx.read().unwrap();
        _UART_BUFFER.get_mut().enqueue(data).unwrap();
    });
}

enum UartErrors {
    EMPTY,
}

fn read_from_uart_buffer() -> Result<u8, UartErrors> {
    let empty = unsafe { _UART_BUFFER.get_mut().is_empty() };
    if empty {
        Err(UartErrors::EMPTY)
    } else {
        let val = unsafe { _UART_BUFFER.get_mut().dequeue().unwrap() };
        Ok(val)
    }
}

fn read_buffer() -> heapless::String<128> {
    let mut data = [0u8; 128];
    let mut i = 0;
    loop {
        match read_from_uart_buffer() {
            Ok(val) => data[i] = val,
            Err(_) => (),
        }
        i += 1;
        if i > 2 {
            break;
        }
    }
    let bytes = from_utf8(&data[0..i]).unwrap();
    heapless::String::from_str(bytes).unwrap()
}

fn setup_uart1_interrupts(rx: Rx<USART1>) {
    unsafe {
        let cs = CriticalSection::new();
        _USART1_RX.borrow(cs).replace(Some(rx));
    }
    ECLIC::reset();
    ECLIC::setup(
        Interrupt::USART1,
        longan_nano::hal::eclic::TriggerType::FallingEdge,
        longan_nano::hal::eclic::Level::L0,
        longan_nano::hal::eclic::Priority::P0,
    );
}

fn enable_interrupts() {
    unsafe {
        interrupt::enable();
    }
}

#[entry]
fn main() -> ! {
    let dp = Peripherals::take().unwrap();
    let mut rcu = dp
        .RCU
        .configure()
        .ext_hf_clock(8.mhz())
        .sysclk(108.mhz())
        .freeze();
    let gpioa = dp.GPIOA.split(&mut rcu);
    let gpiob = dp.GPIOB.split(&mut rcu);
    let lcd_pins = lcd_pins!(gpioa, gpiob);
    let mut afio = dp.AFIO.constrain(&mut rcu);
    let mut lcd = lcd::configure(dp.SPI0, lcd_pins, &mut afio, &mut rcu);

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::GREEN)
        .background_color(Rgb565::BLACK)
        .build();
    let (width, height) = (lcd.size().width as i32, lcd.size().height as i32);

    let tx = gpioa.pa2.into_alternate_push_pull();
    let rx = gpioa.pa3.into_floating_input();

    let uart = Serial::new(dp.USART1, (tx, rx), UART_CONFIG!(), &mut afio, &mut rcu);
    let (mut tx, mut rx) = uart.split();
    // setup_uart1_interrupts(rx);
    // enable_interrupts();

    let mut delay = McycleDelay::new(&rcu.clocks);

    loop {
        tx.write_str("AT+RST\r").unwrap();
        Text::new("0", Point::new(10, 10), style)
            .draw(&mut lcd)
            .unwrap();

        delay.delay_ms(1000);
        Text::new("1", Point::new(10, 10), style)
            .draw(&mut lcd)
            .unwrap();
        delay.delay_ms(1000);
    }
}
