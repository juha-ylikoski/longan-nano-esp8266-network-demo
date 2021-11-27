#![no_std]
#![no_main]

use core::{cell::RefCell, fmt::Write};

use embedded_graphics::prelude::*;
use embedded_graphics::{
    mono_font::{jis_x0201::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use esp8266::Esp8266Error;
use longan_nano::hal::prelude::*;
use longan_nano::{
    hal::{
        delay::McycleDelay,
        gpio::GpioExt,
        pac::Peripherals,
        rcu::RcuExt,
        serial::{Config, Serial},
    },
    lcd, lcd_pins,
};
use panic_halt as _;
use riscv_rt::entry;

mod esp8266;

macro_rules! UART_CONFIG {
    () => {
        Config {
            // baudrate: 115200.bps(),
            // baudrate: 74880.bps(),
            baudrate: 9600.bps(),
            parity: longan_nano::hal::serial::Parity::ParityNone,
            stopbits: longan_nano::hal::serial::StopBits::STOP1,
        }
    };
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
    let delay_clock = McycleDelay::new(&rcu.clocks);
    let mut delay = RefCell::new(delay_clock);

    unsafe {
        riscv::interrupt::disable();
    }

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::GREEN)
        .background_color(Rgb565::BLACK)
        .build();
    let (width, height) = (lcd.size().width as i32, lcd.size().height as i32);

    let tx = gpioa.pa2.into_alternate_push_pull();
    let rx = gpioa.pa3.into_floating_input();

    Rectangle::new(Point::new(0, 0), Size::new(width as u32, height as u32))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(&mut lcd)
        .unwrap();

    let uart = Serial::new(dp.USART1, (tx, rx), UART_CONFIG!(), &mut afio, &mut rcu);
    let (tx, rx) = uart.split();

    let uart2 = Serial::new(
        dp.USART2,
        (
            gpiob.pb10.into_alternate_push_pull(),
            gpiob.pb11.into_floating_input(),
        ),
        UART_CONFIG!(),
        &mut afio,
        &mut rcu,
    );
    let (mut tx2, _) = uart2.split();

    Text::new("Start setup", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    tx2.write_str("Creating ESP8266\r\n").unwrap();
    let mut esp = esp8266::Esp8266::new(rx, tx, RefCell::clone(&delay), tx2);

    Text::new("Setup complete", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    esp.at().unwrap();
    Text::new("Test AT complete", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();
    esp.connect_wifi().unwrap();

    loop {
        match esp.get() {
            Ok(v) => {
                let mut txt = heapless::String::<32>::from("Epoch time: \n");
                let json = v.json;
                txt.push_str(&heapless::String::<16>::from(json)).unwrap();

                Text::new(&txt, Point::new(10, 30), style)
                    .draw(&mut lcd)
                    .unwrap();
            }
            Err(e) => {
                if let Esp8266Error::GetError(v) = e {
                    let mut s = heapless::String::<256>::from("GetError ");
                    s.push_str(&v).unwrap();
                    Text::new(&s, Point::new(10, 30), style)
                        .draw(&mut lcd)
                        .unwrap();
                }
            }
        }
        delay.get_mut().delay_ms(1000);
    }
}
