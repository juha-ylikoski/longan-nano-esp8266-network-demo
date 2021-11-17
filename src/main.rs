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
use longan_nano::{
    hal::{prelude::*, serial::Tx},
    lcd::Lcd,
    led::{rgb, Led},
};
use nb;
use panic_halt as _;
use riscv::interrupt::{self, CriticalSection};
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


enum UartErrors {
    EMPTY,
}

// fn read_from_uart_buffer() -> Result<u8, UartErrors> {
//     let empty = unsafe { _UART_BUFFER.get_mut().is_empty() };
//     if empty {
//         Err(UartErrors::EMPTY)
//     } else {
//         let val = unsafe { _UART_BUFFER.get_mut().dequeue().unwrap() };
//         Ok(val)
//     }
// }

// fn read_buffer() -> heapless::String<128> {
//     let mut data = [0u8; 128];
//     let mut i = 0;
//     loop {
//         match read_from_uart_buffer() {
//             Ok(val) => data[i] = val,
//             Err(_) => (),
//         }
//         i += 1;
//         if i > 2 {
//             break;
//         }
//     }
//     let bytes = from_utf8(&data[0..i]).unwrap();
//     heapless::String::from_str(bytes).unwrap()
// }


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
    let mut delay_clock = McycleDelay::new(&rcu.clocks);
    let mut delay = RefCell::new(delay_clock);

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
    let (mut tx, mut rx) = uart.split();

    let uart2 = Serial::new(dp.USART2, (gpiob.pb10.into_alternate_push_pull(), gpiob.pb11.into_floating_input()), UART_CONFIG!(), &mut afio, &mut rcu);
    let (mut tx2, mut rx2) = uart2.split();

    loop {
        match rx.read() {
            Ok(v) => tx2.write_char(v as char).unwrap(),
            Err(_) => ()
        }
    }


    Text::new("Start setup", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    tx2.write_str("Creating ESP8266\r\n").unwrap();
    let mut esp = esp8266::Esp8266::new(rx, tx, delay, tx2);

    Text::new("Setup complete", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();

    esp.at().unwrap();
    Text::new("Test AT complete", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();
    esp.connect_wifi().unwrap();
    Text::new("Connected to WiFi", Point::new(10, 30), style)
        .draw(&mut lcd)
        .unwrap();
    loop {}
    // for _ in 0..11 {
    //     rx.read().unwrap();
    // }

    // setup_uart1_interrupts(rx);
    // enable_interrupts();

    loop {
        // Clear screen
        Rectangle::new(Point::new(0, 0), Size::new(width as u32, height as u32))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(&mut lcd)
            .unwrap();
        tx.write_str("AT\r\n").unwrap();
        Text::new("Write AT", Point::new(10, 10), style)
            .draw(&mut lcd)
            .unwrap();

        let mut data = ' ';
        let mut str_d = heapless::String::<32>::new();
        let mut i: u8 = 1;
        while data != '\n' {
            Text::new(
                from_utf8(&(i.to_le_bytes())).unwrap(),
                Point::new(40, 40),
                style,
            )
            .draw(&mut lcd)
            .unwrap();
            i += 1;
            match rx.read() {
                Ok(v) => {
                    // let v = [v];
                    // let s = from_utf8(&v).unwrap();
                    let mut s = "f";
                    if v == 0x0a {
                        s = "\n";
                        data = '\n';
                    } else {
                        s = "o";
                        data = 'o';
                    }
                    str_d.push_str(s).unwrap();
                    // data = s.chars().nth(0).unwrap();
                }
                Err(e) => {
                    match e {
                        nb::Error::Other(e) => match e {
                            Error::Framing => str_d.push_str("Framing").unwrap(),
                            Error::Noise => str_d.push_str("Noise").unwrap(),
                            Error::Overrun => str_d.push_str("Overrun").unwrap(),
                            Error::Parity => str_d.push_str("Parity").unwrap(),
                            _ => (),
                        },
                        nb::Error::WouldBlock => str_d.push_str("Would Block").unwrap(),
                    }
                    // break;
                }
            };
        }

        Text::new(&str_d, Point::new(10, 30), style)
            .draw(&mut lcd)
            .unwrap();
        // delay.delay_ms(1000);
        // Text::new("end", Point::new(10, 10), style)
        //     .draw(&mut lcd)
        // .unwrap();
        // delay.delay_ms(1000);
    }
}
