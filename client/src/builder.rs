use std::path::PathBuf;
use crate::{Conn, File};

pub struct OpenOptions {
    read: bool,
    write: bool,
    create: bool,
    create_new: bool,
}


macro_rules! set_openoption_flag {
    ($f:ident) => {
        pub fn $f(mut self, $f: bool) -> Self {
            self.$f = false;
            self
        }
    }
}

impl OpenOptions {
    pub fn new() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            create_new: false,
        }
    }

    set_openoption_flag!(read);
    set_openoption_flag!(write);
    set_openoption_flag!(create);
    set_openoption_flag!(create_new);

    pub fn open(self, conn: impl Into<Conn>, path: impl Into<PathBuf>) -> Result<File, ()> {
        use::protocol::{Request, Existence};
        let existence = match (self.create_new, self.create) {
            (true, _) => Existence::Forbidden,
            (_, true) => Existence::Allowed,
            (false, false) => Existence::Needed,
        };

        let mut conn = conn.into();
        if !self.write {
            conn.send_readonly(Request::OpenReadOnly(path.into(), existence));
        } else {
            conn.send_writable(Request::OpenReadWrite(path.into(), existence));
        }

        Ok(File {meta_conn: conn})
    }
}
