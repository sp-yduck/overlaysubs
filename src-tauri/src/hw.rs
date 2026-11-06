use overlaysubs_core::tier::{Gpu, Hardware};
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let o = c.output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn nvidia() -> Option<Gpu> {
    let out = run("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])?;
    let (name, mb) = out.lines().next()?.rsplit_once(',')?;
    Some(Gpu { name: name.trim().into(), vram_mb: mb.trim().parse().ok()?, unified: false })
}

/// Win32_VideoController.AdapterRAM caps at 4 GB, so read the driver's 64-bit value instead.
#[cfg(windows)]
fn windows_gpu() -> Option<Gpu> {
    let ps = r"Get-ItemProperty 'HKLM:\SYSTEM\ControlSet001\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' -ErrorAction SilentlyContinue | ForEach-Object { '{0}|{1}' -f $_.DriverDesc, $_.'HardwareInformation.qwMemorySize' }";
    let out = run("powershell", &["-NoProfile", "-Command", ps])?;
    out.lines()
        .filter_map(|l| {
            let (n, m) = l.split_once('|')?;
            Some(Gpu { name: n.into(), vram_mb: m.trim().parse::<u64>().ok()? / (1024 * 1024), unified: false })
        })
        .max_by_key(|g| g.vram_mb)
}

#[cfg(not(windows))]
fn windows_gpu() -> Option<Gpu> {
    None
}

pub fn detect() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let ram_mb = sys.total_memory() / (1024 * 1024);
    let gpu = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Gpu { name: "Apple Silicon".into(), vram_mb: ram_mb, unified: true })
    } else {
        nvidia().or_else(windows_gpu)
    };
    Hardware {
        gpu,
        ram_mb,
        cpu_threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
        cpu_name: sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
    }
}
