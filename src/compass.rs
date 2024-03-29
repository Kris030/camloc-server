use std::{future::Future, pin::Pin};

#[macro_export]
macro_rules! no_compass {
    () => {
        if true {
            None
        } else {
            struct NoCompass;
            impl Compass for NoCompass {
                fn get_value(
                    &mut self,
                ) -> ::std::pin::Pin<
                    Box<dyn ::std::future::Future<Output = Option<f64>> + Send + Sync>,
                > {
                    unreachable!()
                }

                fn stop(
                    self,
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send + Sync>>
                {
                    unreachable!()
                }

                fn stop_box(
                    self: Box<Self>,
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send + Sync>>
                {
                    unreachable!()
                }
            }

            Some(NoCompass)
        }
    };
}

pub use no_compass;
pub trait Compass: Send + Sync {
    fn get_value(&mut self) -> Pin<Box<dyn Future<Output = Option<f64>> + Send + Sync>>;
    fn stop(self) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>>;
    fn stop_box(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>>;
}

#[cfg(feature = "serial-compass")]
pub mod serial {
    use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
    use tokio::{sync::RwLock, task::JoinHandle};

    use super::Compass;

    struct SharedData {
        last_value: RwLock<Option<f64>>,
        compass_offset: RwLock<f64>,
        run: RwLock<bool>,
    }

    pub struct SerialCompass {
        port_name: String,
        shared: Arc<SharedData>,
        handle: JoinHandle<()>,
    }

    impl SerialCompass {
        pub fn start(
            mut serial_port: tokio_serial::SerialStream,
            update_interval: Duration,
            compass_offset: f64,
            port_name: String,
        ) -> SerialCompass {
            let details = Arc::new(SharedData {
                compass_offset: RwLock::const_new(compass_offset),
                last_value: RwLock::const_new(None),
                run: RwLock::const_new(true),
            });
            let details2 = details.clone();

            let handle = tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut i = tokio::time::interval(update_interval);
                loop {
                    i.tick().await;

                    let r = details2.run.read().await;
                    if !*r {
                        break;
                    }
                    drop(r);

                    if serial_port.write_all(&Self::DATA_SIGNAL).await.is_err() {
                        *details2.last_value.write().await = None;
                        continue;
                    }

                    let offset_handle = details2.compass_offset.read().await;
                    let offset = *offset_handle;
                    drop(offset_handle);

                    let value = serial_port.read_f64().await.ok().map(|v| v - offset);

                    let mut last_value = details2.last_value.write().await;
                    *last_value = value;
                    drop(last_value);
                }
            });

            Self {
                shared: details,
                port_name,
                handle,
            }
        }

        pub async fn set_offset(&self, new_offset: f64) {
            *self.shared.compass_offset.write().await = new_offset;
        }

        pub fn get_port_name(&self) -> &str {
            &self.port_name
        }

        pub const DATA_SIGNAL: [u8; 1] = [b'$'];
    }

    impl Compass for SerialCompass {
        fn get_value(&mut self) -> Pin<Box<dyn Future<Output = Option<f64>> + Send + Sync>> {
            let dets = self.shared.clone();
            Box::pin(async move { *dets.last_value.read().await })
        }

        fn stop(self) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
            Box::pin(async move {
                let mut r = self.shared.run.write().await;
                *r = false;
                drop(r);
                self.handle.await.unwrap();
            })
        }

        fn stop_box(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
            self.stop()
        }
    }
}
