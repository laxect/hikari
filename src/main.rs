use std::thread::sleep;
use std::time::Duration;

use zbus::{blocking::Connection, dbus_proxy};

#[dbus_proxy(
    interface = "net.hadess.SensorProxy",
    default_service = "net.hadess.SensorProxy",
    default_path = "/net/hadess/SensorProxy"
)]
trait Sensors {
    fn claim_light(&self) -> zbus::Result<()>;
    #[dbus_proxy(property)]
    fn light_level(&self) -> zbus::Result<f64>;
}

fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let conn = Connection::system()?;
    let proxy = SensorsProxyBlocking::new(&conn)?;

    loop {
        proxy.claim_light()?;
        let level = proxy.light_level()?;

        let now = chrono::Local::now();

        println!("{},{:04} lux", now.time(), level.floor() as u64);

        sleep(Duration::new(30, 0));
    }
}
