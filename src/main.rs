use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{self, Duration};

use color_eyre::eyre;
use tracing::{error, info};
use zbus::{blocking::Connection, dbus_proxy};

/// # brightness
/// 40%
const MAX: f64 = 3000.0;
/// 25%
const MID: f64 = 2000.0;
/// 15%
const MIN: f64 = 1125.0;

/// # environment light
/// Noon
const MAX_LUX: f64 = 2500.0;
/// Night
const MIN_LUX: f64 = 400.0;

static TARGET: AtomicU32 = AtomicU32::new(0);

/// compute brightness by environment light level.
fn lux_to_brightness(lux: f64) -> u32 {
    if lux > MAX_LUX {
        return MAX as u32;
    }
    if lux <= MIN_LUX {
        return MIN as u32;
    }
    // (0, 1]
    let t = (lux - MIN_LUX) / (MAX_LUX - MIN_LUX);
    let p = (1.0 - t).powi(2) * MIN + 2.0 * t * (1.0 - t) * MID + t.powi(2) * MAX;
    p.floor() as u32
}

#[dbus_proxy(
    interface = "org.freedesktop.login1.Session",
    default_path = "/org/freedesktop/login1/session/auto",
    default_service = "org.freedesktop.login1"
)]
trait Login1 {
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> zbus::Result<()>;
}

#[dbus_proxy(
    interface = "net.hadess.SensorProxy",
    default_path = "/net/hadess/SensorProxy",
    default_service = "net.hadess.SensorProxy"
)]
trait Sensors {
    fn claim_light(&self) -> zbus::Result<()>;
    #[dbus_proxy(property)]
    fn light_level(&self) -> zbus::Result<f64>;
}

/// Get light level from iio proxy (hadess).
fn moniter_lux(hadess: &SensorsProxyBlocking) -> eyre::Result<()> {
    let mut now = time::Instant::now();

    loop {
        // check if The System has been suspend since last run, by
        // simply check the time elapsed.
        let dur = now.elapsed();
        if dur > Duration::new(20, 0) {
            info!("time warp found.");
            thread::sleep(Duration::new(20, 0));
        }
        now = time::Instant::now();

        hadess.claim_light()?;
        let level = hadess.light_level()?;

        let now = chrono::Local::now();
        info!("{},{:04} lux", now.time(), level.floor() as u64);

        TARGET.store(lux_to_brightness(level), Ordering::Release);

        thread::sleep(Duration::new(5, 0));
    }
}

/// Use the freedesktop api to set brightness.
fn set_brightness(login1: &Login1ProxyBlocking) -> eyre::Result<()> {
    let mut now: Option<u32> = Option::None;
    loop {
        thread::sleep(Duration::new(0, 50_000_000));

        let target = TARGET.load(Ordering::Acquire);
        let new = if let Some(now) = now {
            if now > target {
                if (now - target) < 10 {
                    continue;
                }
                now - 10
            } else {
                if (target - now) < 10 {
                    continue;
                }
                now + 10
            }
        } else {
            target
        };
        login1.set_brightness("backlight", "intel_backlight", new)?;
        now = Some(new);
    }
}

fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    // zbus connection is send+sync, so we just use one in two thread.
    let conn = Connection::system()?;
    let hadess = SensorsProxyBlocking::new(&conn)?;
    let login1 = Login1ProxyBlocking::new(&conn)?;

    // set up
    hadess.claim_light()?;
    TARGET.store(lux_to_brightness(hadess.light_level()?), Ordering::Release);

    let moniter_t = thread::spawn(move || loop {
        if let Err(e) = moniter_lux(&hadess) {
            error!("Moniter: {}", e);
            thread::sleep(Duration::new(10, 0));
        }
    });
    let update_t = thread::spawn(move || loop {
        if let Err(e) = set_brightness(&login1) {
            error!("Login1: {}", e);
            thread::sleep(Duration::new(10, 0));
        }
    });

    moniter_t.join().unwrap();
    update_t.join().unwrap();
    Ok(())
}
