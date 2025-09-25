// Systray Lib
pub mod api;

use std::{
    collections::HashMap,
    error, fmt,
    sync::mpsc::{channel, Receiver},
};

type BoxedError = Box<dyn error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum Error {
    OsError(String),
    NotImplementedError,
    UnknownError,
    Error(BoxedError),
}

impl From<BoxedError> for Error {
    fn from(value: BoxedError) -> Self {
        Error::Error(value)
    }
}

pub struct SystrayEvent {
    menu_index: u32,
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use self::Error::*;

        match *self {
            OsError(ref err_str) => write!(f, "OsError: {}", err_str),
            NotImplementedError => write!(f, "Functionality is not implemented yet"),
            UnknownError => write!(f, "Unknown error occurrred"),
            Error(ref e) => write!(f, "Error: {}", e),
        }
    }
}

pub struct Application {
    window: api::platform::Window,
    menu_idx: u32,
    callback: HashMap<u32, Callback>,
    // Each platform-specific window module will set up its own thread for
    // dealing with the OS main loop. Use this channel for receiving events from
    // that thread.
    rx: Receiver<SystrayEvent>,
}

type Callback =
    Box<dyn FnMut(&mut Application) -> Result<(), BoxedError> + Send + Sync + 'static>;

fn make_callback<F, E>(mut f: F) -> Callback
where
    F: FnMut(&mut Application) -> Result<(), E> + Send + Sync + 'static,
    E: error::Error + Send + Sync + 'static,
{
    Box::new(move |a: &mut Application| match f(a) {
        Ok(()) => Ok(()),
        Err(e) => Err(Box::new(e) as BoxedError),
    }) as Callback
}

impl Application {
    pub fn new() -> Result<Application, Error> {
        let (event_tx, event_rx) = channel();
        match api::platform::Window::new(event_tx) {
            Ok(w) => Ok(Application {
                window: w,
                menu_idx: 0,
                callback: HashMap::new(),
                rx: event_rx,
            }),
            Err(e) => Err(e),
        }
    }

    pub fn add_menu_item<F, E>(&mut self, item_name: &str, f: F) -> Result<u32, Error>
    where
        F: FnMut(&mut Application) -> Result<(), E> + Send + Sync + 'static,
        E: error::Error + Send + Sync + 'static,
    {
        let idx = self.menu_idx;
        self.window.add_menu_entry(idx, item_name)?;
        self.callback.insert(idx, make_callback(f));
        self.menu_idx += 1;
        Ok(idx)
    }

    pub fn add_menu_separator(&mut self) -> Result<u32, Error> {
        let idx = self.menu_idx;
        self.window.add_menu_separator(idx)?;
        self.menu_idx += 1;
        Ok(idx)
    }

    pub fn set_icon_from_file(&self, file: &str) -> Result<(), Error> {
        self.window.set_icon_from_file(file)
    }

    pub fn set_icon_from_resource(&self, resource: &str) -> Result<(), Error> {
        self.window.set_icon_from_resource(resource)
    }

    pub fn set_icon_from_image_file(&self, file: &str) -> Result<(), Error> {
        use image::io::Reader as ImageReader;
        use std::path::Path;
        
        let path = Path::new(file);
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        match extension.as_str() {
            "png" | "jpg" | "jpeg" => {
                // 对于PNG和JPG格式，尝试直接加载
                match self.window.set_icon_from_file(file) {
                    Ok(()) => Ok(()),
                    Err(_) => {
                        // 如果平台不支持，转换为平台支持的格式
                        let img = ImageReader::open(path)
                            .map_err(|e| Error::OsError(format!("Failed to open image: {}", e)))?
                            .decode()
                            .map_err(|e| Error::OsError(format!("Failed to decode image: {}", e)))?;
                        
                        let (width, height) = (img.width(), img.height());
                        let rgba_img = img.to_rgba8();
                        let buffer = rgba_img.into_raw();
                        
                        #[cfg(target_os = "windows")]
                         {
                             // Windows: 转换为ICO格式或位图
                             self.window.set_icon_from_buffer(&buffer, width, height)
                         }
                         
                         #[cfg(target_os = "linux")]
                         {
                             // Linux: GTK支持PNG，JPG需要转换
                             self.window.set_icon_from_image_buffer(&buffer, width, height)
                         }
                         
                         #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                         {
                             // 其他平台：需要实现
                             Err(Error::NotImplementedError)
                         }
                    }
                }
            }
            "ico" | "bmp" => {
                // 对于ICO和BMP格式，使用原有的方法
                self.window.set_icon_from_file(file)
            }
            _ => Err(Error::OsError(format!("Unsupported image format: {}", extension))),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn set_icon_from_buffer(
        &self,
        buffer: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        self.window.set_icon_from_buffer(buffer, width, height)
    }

    pub fn shutdown(&self) -> Result<(), Error> {
        self.window.shutdown()
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), Error> {
        self.window.set_tooltip(tooltip)
    }

    pub fn quit(&mut self) {
        self.window.quit()
    }

    pub fn wait_for_message(&mut self) -> Result<(), Error> {
        loop {
            
            let msg = match self.rx.recv() {
                Ok(m) => m,
                Err(_) => {
                    self.quit();
                    break;
                }
            };
            if self.callback.contains_key(&msg.menu_index) {
                if let Some(mut f) = self.callback.remove(&msg.menu_index) {
                    f(self)?;
                    self.callback.insert(msg.menu_index, f);
                }
            }
        }

        Ok(())
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.shutdown().ok();
    }
}
