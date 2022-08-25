use std::sync::mpsc::{Receiver, Sender};
use std::time::{self, Duration};
use std::{sync, thread};

use color_eyre::eyre;
use tracing::{debug, error, info};
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
fn moniter_lux(s: Sender<u32>) -> eyre::Result<()> {
    let connection = Connection::system()?;
    let hadess = SensorsProxyBlocking::new(&connection)?;

    // the first claim
    hadess.claim_light()?;
    info!("first claim");
    thread::sleep(Duration::from_secs(5));

    let mut now = time::SystemTime::now();
    loop {
        // check if The System has been suspend since last run, by
        // simply check the time elapsed.
        let dur = now.elapsed().unwrap_or_default();
        debug!("time pass {:#?}", dur);
        if dur > Duration::from_secs(20) {
            info!("time warp detected.");
            return Ok(());
        }

        // there is, still chance, things will broken.
        // but I think the defence is enough.
        hadess.claim_light()?;
        let level = hadess.light_level()?;

        let ima = chrono::Local::now();
        debug!("{},{:04} lux", ima.time(), level.floor() as u64);

        s.send(lux_to_brightness(level))?;

        now = time::SystemTime::now();
        debug!("now is {:?}", now);
        thread::sleep(Duration::from_secs(5));
    }
}

/// Use the freedesktop api to set brightness.
fn set_brightness(r: Receiver<u32>) -> eyre::Result<()> {
    let connection = Connection::system()?;
    let login1 = Login1ProxyBlocking::new(&connection)?;

    let mut now: Option<u32> = Option::None;
    let mut target = r.recv()?;
    loop {
        thread::sleep(Duration::from_nanos(50_000_000));

        if let Ok(new_target) = r.try_recv() {
            debug!("update target: {}", new_target);
            target = new_target;
        }

        let new = if let Some(now) = now {
            if now.abs_diff(target) < 10 {
                continue;
            }
            if now > target {
                now - 10
            } else {
                now + 10
            }
        } else {
            target
        };

        // absolute value. no need to fix.
        login1
            .set_brightness("backlight", "intel_backlight", new)
            .map_err(|e| error!("Login1: {}", e))
            .ok();
        now = Some(new);
    }
}

fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let (s, r) = sync::mpsc::channel::<u32>();

    let _moniter_t = thread::spawn(move || loop {
        let s = s.clone();
        if let Err(e) = moniter_lux(s) {
            error!("Moniter: {}", e);
        }
        thread::sleep(Duration::from_secs(10));
        info!("next round!");
    });
    let update_t = thread::spawn(move || set_brightness(r));

    update_t.join().unwrap().unwrap();
    Ok(())
}
