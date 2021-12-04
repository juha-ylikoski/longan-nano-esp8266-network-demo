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
use longan_nano::lcd::Lcd;
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

fn clear_screen(lcd: &mut Lcd) {
    Rectangle::new(
        Point::new(0, 0),
        Size::new(lcd.size().width, lcd.size().height),
    )
    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
    .draw(lcd)
    .unwrap();
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

    let tx = gpioa.pa2.into_alternate_push_pull();
    let rx = gpioa.pa3.into_floating_input();

    clear_screen(&mut lcd);

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
    clear_screen(&mut lcd);
    Text::new("Start setup", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    clear_screen(&mut lcd);
    tx2.write_str("Creating ESP8266\r\n").unwrap();
    let mut esp = esp8266::Esp8266::new(rx, tx, RefCell::clone(&delay), tx2);
    clear_screen(&mut lcd);
    Text::new("Setup complete", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    match esp.at() {
        Ok(_) => {
            clear_screen(&mut lcd);
            Text::new("Test AT complete", Point::new(10, 30), style)
                .draw(&mut lcd)
                .unwrap();
        }
        Err(_) => {
            clear_screen(&mut lcd);
            Text::new("Test AT Failed", Point::new(10, 30), style)
                .draw(&mut lcd)
                .unwrap();
            panic!("AT failed");
        }
    }
    delay.get_mut().delay_ms(100);
    clear_screen(&mut lcd);
    Text::new("Connecting to wifi", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();
    match esp.connect_wifi() {
        Ok(_) => {
            clear_screen(&mut lcd);
            Text::new("WiFi OK", Point::new(10, 30), style)
                .draw(&mut lcd)
                .unwrap();
        }
        Err(_) => {
            clear_screen(&mut lcd);
            Text::new("WiFi Failed", Point::new(10, 30), style)
                .draw(&mut lcd)
                .unwrap();
        }
    }

    loop {
        match esp.get() {
            Ok(v) => {
                clear_screen(&mut lcd);
                let mut txt = heapless::String::<32>::from("CPU: ");
                let json = v.json;
                let _cpu_usage = json[0];
                let _cpu0_temp = json[1];
                let _cpu1_temp = json[2];
                let _cpu2_temp = json[3];
                let _cpu3_temp = json[4];
                let _package0_temp = json[5];
                txt.push_str(&heapless::String::<16>::from(_cpu_usage))
                    .unwrap();
                txt.push_str("%\nCore0: ").unwrap();
                txt.push_str(&heapless::String::<16>::from(_cpu0_temp))
                    .unwrap();
                txt.push_str("C").unwrap();

                Text::new(&txt, Point::new(10, 30), style)
                    .draw(&mut lcd)
                    .unwrap();
            }
            Err(e) => {
                clear_screen(&mut lcd);
                match e {
                    Esp8266Error::Error(_) => {
                        Text::new("Generic error", Point::new(10, 30), style)
                            .draw(&mut lcd)
                            .unwrap();
                    }
                    Esp8266Error::FmtError(_) => {
                        Text::new("FmtError", Point::new(10, 30), style)
                            .draw(&mut lcd)
                            .unwrap();
                    }
                    Esp8266Error::GetError(_) => {
                        Text::new("GetError", Point::new(10, 30), style)
                            .draw(&mut lcd)
                            .unwrap();
                    }
                    Esp8266Error::JsonError => {
                        Text::new("Json Error", Point::new(10, 30), style)
                            .draw(&mut lcd)
                            .unwrap();
                    }
                }
            }
        }
        delay.get_mut().delay_ms(1000);
    }
}
