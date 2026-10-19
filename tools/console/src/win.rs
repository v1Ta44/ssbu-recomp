//! Small Win32 pieces: processor affinity (exclude_cpus, inherited by the game), ending the game, opening Explorer.
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask, OpenProcess, SetProcessAffinityMask, TerminateProcess, PROCESS_TERMINATE};

/// Removes the listed logical processors from this process's mask (the child inherits it). Returns (old, new).
pub fn exclude_cpus(list: &str) -> Option<(usize, usize)> {
    let mut proc_mask = 0usize;
    let mut sys_mask = 0usize;
    unsafe {
        if GetProcessAffinityMask(GetCurrentProcess(), &mut proc_mask, &mut sys_mask) == 0 {
            return None;
        }
    }
    let mut mask = proc_mask;
    for c in list.split(',') {
        if let Ok(n) = c.trim().parse::<u32>() {
            if n < usize::BITS {
                mask &= !(1usize << n);
            }
        }
    }
    if mask == 0 || mask == proc_mask {
        return None;
    }
    set_affinity(mask);
    Some((proc_mask, mask))
}

pub fn set_affinity(mask: usize) {
    unsafe {
        SetProcessAffinityMask(GetCurrentProcess(), mask);
    }
}

pub fn terminate(pid: u32) -> Result<(), String> {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return Err("进程无法打开".into());
        }
        let ok = TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        if ok {
            Ok(())
        } else {
            Err("进程没有结束".into())
        }
    }
}

pub fn reveal(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let mut arg = std::ffi::OsString::from("/select,\"");
    arg.push(path.as_os_str());
    arg.push("\"");
    let _ = std::process::Command::new("explorer.exe").raw_arg(arg).spawn();
}

// Explorer needs a normal absolute Windows path: mixed ../ separators and the
// verbatim prefixes returned by canonicalize can send it to its default folder.
fn explorer_folder_path(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::path::{Component, PathBuf, Prefix};
    let absolute = std::path::absolute(path)?;
    let mut components = absolute.components();
    let Some(Component::Prefix(prefix)) = components.next() else { return Ok(absolute); };
    let mut normal = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut root = PathBuf::from(r"\\");
            root.push(server);
            root.push(share);
            root
        }
        _ => return Ok(absolute),
    };
    normal.extend(components);
    std::path::absolute(normal)
}

pub fn open_folder(path: &std::path::Path) -> Result<(), String> {
    let path = explorer_folder_path(path).map_err(|e| format!("无法解析文件夹路径：{e}"))?;
    std::process::Command::new("explorer.exe").arg(path).spawn()
        .map_err(|e| format!("无法打开文件夹：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod folder_tests {
    use super::*;
    #[test]
    fn mod_folder_paths_are_shell_compatible() {
        for (input, expected) in [
            (r"D:\测试 包\game\../mods", r"D:\测试 包\mods"),
            (r"\\?\D:\测试 包\mods\纳克鲁斯", r"D:\测试 包\mods\纳克鲁斯"),
            (r"\\?\UNC\server\share\mods\中文 皮肤", r"\\server\share\mods\中文 皮肤"),
            (r"\\server\share\game\../mods", r"\\server\share\mods"),
            (r"E:/自定义资源/../中文 模组", r"E:\中文 模组"),
        ] {
            let actual = explorer_folder_path(std::path::Path::new(input)).unwrap();
            assert_eq!(actual.as_os_str(), std::ffi::OsStr::new(expected), "{input}");
        }
    }
}
