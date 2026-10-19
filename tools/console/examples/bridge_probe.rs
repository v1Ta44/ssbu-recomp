//! Drives runtime/console_bridge.c in a real game exe without booting the game: poll, log level, a join to a port where
//! no room server listens (it must fail with a timeout, not hang), leave, then closing the pipe before "start" must end
//! the process with exit code 0. Usage: cargo run --example bridge_probe -- <path to exe>
#[path = "../src/bridge.rs"]
mod bridge;
use std::time::{Duration, Instant};

fn main() {
    let exe = std::env::args().nth(1).expect("exe path");
    let b = bridge::Bridge::new();
    let name = b.listen().unwrap();
    let mut child = std::process::Command::new(&exe)
        .current_dir(std::path::Path::new(&exe).parent().unwrap())
        .env("SSBU_CONSOLE_PIPE", &name)
        .env("SSBU_NET_LOG_LEVEL", "1")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    assert!(b.accept(Duration::from_secs(20)), "the game did not connect");
    let p = b.call("poll").unwrap();
    println!("poll: state {} started {} message {}", p["state"], p["started"], p["message"]);
    assert_eq!(p["state"], 0);
    assert_eq!(p["started"], false);
    println!("level: {}", b.call("level\t2").unwrap());
    assert_eq!(b.call("level\t9").unwrap()["code"], 1, "level 9 must be refused");
    let bad = b.call(&format!("join\t{}\t24872\t{}\t\t", bridge::hex("127.0.0.1"), bridge::hex("ab"))).unwrap();
    println!("join with a 2-letter nickname: {}", bad);
    assert_eq!(bad["code"], 1);
    let j = b.call(&format!("join\t{}\t1\t{}\t{}\t", bridge::hex("127.0.0.1"), bridge::hex("PCPlayer"), bridge::hex("pässwörd"))).unwrap();
    println!("join: {}", j);
    assert_eq!(j["code"], 0);
    let t0 = Instant::now();
    let mut last = -1i64;
    loop {
        let p = b.call("poll").unwrap();
        let s = p["state"].as_i64().unwrap();
        if s != last {
            println!("  {:>5} ms state {} error {} {}", t0.elapsed().as_millis(), s, p["error"], p["message"]);
            last = s;
        }
        if s == 4 || t0.elapsed() > Duration::from_secs(20) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(last, 4, "a join without a server must fail");
    println!("leave: {}", b.call("leave").unwrap());
    let p = b.call("poll").unwrap();
    println!("after leave: state {} log chars {}", p["state"], p["log"].as_str().unwrap_or("").len());
    assert!(!p["log"].as_str().unwrap().contains("pässwörd"));
    b.close();
    let t1 = Instant::now();
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        assert!(t1.elapsed() < Duration::from_secs(10), "the game did not end after the console closed");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    for l in out.lines().filter(|l| l.contains("console bridge")) {
        println!("game: {}", l);
    }
    println!("exit code {:?}", code);
    assert_eq!(code, Some(0));
    assert!(!out.contains("pässwörd"));
    println!("PASS");
}
