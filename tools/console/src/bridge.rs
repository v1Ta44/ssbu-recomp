//! The console's end of the pipe to the game (runtime/console_bridge.c). One request line, one JSON line back.
//! The console creates the pipe before it starts the game and names it in SSBU_CONSOLE_PIPE; the game connects at boot,
//! before it creates any socket, and waits for "start". Every call is serialised by one mutex.
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};

struct Pipe {
    h: HANDLE,
    connected: bool,
}
unsafe impl Send for Pipe {}

pub struct Bridge {
    pipe: Mutex<Option<Pipe>>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{:02x}", b)).collect()
}

impl Bridge {
    pub fn new() -> Bridge {
        Bridge { pipe: Mutex::new(None) }
    }

    /// A new pipe for the next run; returns its name for SSBU_CONSOLE_PIPE.
    pub fn listen(&self) -> Result<String, String> {
        self.close();
        let name = format!(r"\\.\pipe\ssbu-console-{}-{}", std::process::id(), chrono::Local::now().timestamp_millis());
        let w = wide(&name);
        let h = unsafe {
            CreateNamedPipeW(
                w.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                1 << 20,
                1 << 16,
                0,
                std::ptr::null(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(format!("CreateNamedPipe error {}", unsafe { GetLastError() }));
        }
        *self.pipe.lock().unwrap() = Some(Pipe { h, connected: false });
        Ok(name)
    }

    /// Waits for the game to connect. ConnectNamedPipe blocks, so it runs on a thread of its own and is abandoned at the
    /// time limit (close() then breaks it). true = connected.
    pub fn accept(&self, limit: Duration) -> bool {
        let h = match self.pipe.lock().unwrap().as_ref() {
            Some(p) => p.h as usize,
            None => return false,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ok = unsafe { ConnectNamedPipe(h as HANDLE, std::ptr::null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            let _ = tx.send(ok);
        });
        let ok = rx.recv_timeout(limit).unwrap_or(false);
        if let Some(p) = self.pipe.lock().unwrap().as_mut() {
            p.connected = ok;
        }
        ok
    }

    pub fn connected(&self) -> bool {
        self.pipe.lock().unwrap().as_ref().map_or(false, |p| p.connected)
    }

    pub fn abandon(&self) {
        self.close();
    }

    pub fn close(&self) {
        if let Some(p) = self.pipe.lock().unwrap().take() {
            unsafe {
                DisconnectNamedPipe(p.h);
                CloseHandle(p.h);
            }
        }
    }

    /// One request. Fields are joined with tabs by the caller (strings hex-encoded).
    pub fn call(&self, line: &str) -> Result<Value, String> {
        let mut guard = self.pipe.lock().unwrap();
        let p = guard.as_mut().filter(|p| p.connected).ok_or("游戏没有连上控制台 (未运行, 或已结束)")?;
        let mut req = line.as_bytes().to_vec();
        req.push(b'\n');
        let mut done = 0usize;
        while done < req.len() {
            let mut n = 0u32;
            if unsafe { WriteFile(p.h, req[done..].as_ptr(), (req.len() - done) as u32, &mut n, std::ptr::null_mut()) } == 0 {
                p.connected = false;
                return Err("与游戏的连接已断开".into());
            }
            done += n as usize;
        }
        for b in req.iter_mut() {
            *b = 0; // a join line holds the password
        }
        let mut out: Vec<u8> = Vec::new();
        let end = Instant::now() + Duration::from_secs(5);
        let mut buf = [0u8; 8192];
        loop {
            let mut avail = 0u32;
            if unsafe { PeekNamedPipe(p.h, std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut avail, std::ptr::null_mut()) } == 0 {
                p.connected = false;
                return Err("与游戏的连接已断开".into());
            }
            if avail == 0 {
                if Instant::now() > end {
                    return Err("游戏没有回答 (5 s)".into());
                }
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            let mut n = 0u32;
            let want = (avail as usize).min(buf.len()) as u32;
            if unsafe { ReadFile(p.h, buf.as_mut_ptr(), want, &mut n, std::ptr::null_mut()) } == 0 {
                p.connected = false;
                return Err("与游戏的连接已断开".into());
            }
            out.extend_from_slice(&buf[..n as usize]);
            if out.last() == Some(&b'\n') {
                break;
            }
        }
        let text = String::from_utf8_lossy(&out);
        serde_json::from_str(text.trim()).map_err(|e| format!("游戏的回答无法解析: {}", e))
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.close();
    }
}
