use sysinfo::{Components, Disks, Motherboard, NetworkData, Networks, RefreshKind, System};
use nvml_wrapper::{enum_wrappers::device::{Clock, PcieUtilCounter, TemperatureSensor}, Nvml};
use std::{cmp::Reverse, fmt::Write};

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const SKY: &str = "\x1b[96m";
const BLUE: &str = "\x1b[94m";
const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";

#[must_use]
fn format_size(bytes: u64) -> String {
    let bytes_f = bytes as f64;
    let kib = 1u64 << 10; // 1 KiB
    let mib = 1u64 << 20; // 1 MiB
    let gib = 1u64 << 30; // 1 GiB
    let tib = 1u64 << 40; // 1 TiB
    
    if bytes < kib {
        format!("{bytes}B")
    } else if bytes < mib {
        let kib_val = bytes_f / kib as f64;
        format!("{kib_val:.0}K")
    } else if bytes < gib {
        let mib_val = bytes_f / mib as f64;
        format!("{mib_val:.0}M")
    } else if bytes < tib {
        if bytes >= 100 * gib {
            let gib_val = bytes_f / gib as f64;
            format!("{gib_val:.0}G")
        } else {
            let gib_val = (bytes_f / gib as f64 * 10.0).round() / 10.0;
            format!("{gib_val}G")
        }
    } else {
        let tib_val = bytes_f / tib as f64;
        if bytes >= 100 * tib {
            format!("{tib_val:.0}T")
        } else {
            format!("{tib_val:.1}T")
        }
    }
}

fn mem_usage(used: u64, total: u64) -> String {
    let used_str = format_size(used);
    let total_str = format_size(total);
    let percent = ((used as f64 / total as f64) * 100.0).round() as u32;
    format!("{}{used_str}{RESET}/{}{total_str}{RESET}", percent_col(percent), percent_col(percent))
}

#[must_use]
fn mhz(clock: u32, max_clock: u32) -> String {
    let ratio = clock as f32 / max_clock as f32;
    let ratio = ratio * ratio; // better colors
    let percent = (ratio * 100.0).round() as u32;
    let col = percent_col(percent);
    format!(" {col}{}{RESET}", percent_bar(percent))
}

#[must_use]
fn sized_rows(rows: &[String], sizes: &[usize]) -> String {
    let cols = rows.iter().map(|r| r.split(";").collect::<Vec<_>>()).collect::<Vec<_>>();
    if cols.is_empty() {
        return Default::default();
    }
    assert_eq!(cols[0].len(), sizes.len());
    let mut out = String::new();
    for row in cols {
        for (i, item) in row.into_iter().enumerate() {
            out += &format!("{item:<width$} ", width = sizes[i])
        }
        out.push('\n');
    }
    out
}

#[must_use]
fn rows(rows: &[String]) -> String {
    let cols = rows.iter().map(|r| r.split(";").collect::<Vec<_>>()).collect::<Vec<_>>();
    if cols.is_empty() {
        return Default::default();
    }
    let max_lens: Vec<usize> = (0..cols[0].len())
        .map(|i| cols.iter().map(|row| row[i].len()).max().unwrap_or(0))
        .collect();
    sized_rows(rows, &max_lens)
}

fn percent_col(percent: u32) -> &'static str {
    match percent {
        0..=12 => BLUE,
        13..=25 => BLUE,
        26..=37 => SKY,
        38..=50 => SKY,
        51..=62 => MAGENTA,
        63..=75 => MAGENTA,
        76..=87 => RED,
        _ => RED,
    }
}

fn percent_bar(percent: u32) -> &'static str {
    match percent {
        0..=12 => "▁",
        13..=25 => "▂",
        26..=37 => "▃",
        38..=50 => "▄",
        51..=62 => "▅",
        63..=75 => "▆",
        76..=87 => "▇",
        _ => "█",
    }
}

fn percent_slider(percent: u32) -> &'static str {
    match percent {
        0..=12 => "▏",
        13..=25 => "▎",
        26..=37 => "▍",
        38..=50 => "▌",
        51..=62 => "▋",
        63..=75 => "▊",
        76..=87 => "▉",
        _ => "█",
    }
}

#[must_use]
fn mem_bar(used: u64, total: u64, width: u32) -> String {
    let ratio = (used as f64 / total as f64).min(1.0);
    let full_blocks = (ratio * width as f64) as u32;
    let col = percent_col((ratio * 100.0).round() as u32);
    let bar = "█".repeat(full_blocks as usize);
    let usage = mem_usage(used, total);
    if full_blocks == width {
        return format!("[{col}{bar}{RESET}] {usage}");
    }
    let remainder_percent = ((ratio * width as f64).fract() * 100.0).round() as u32;
    let remainder = percent_slider(remainder_percent);
    let empty = &" ".repeat((width - full_blocks - 1) as usize);
    format!("[{col}{bar}{remainder}{RESET}{empty}] {usage}")
}

#[must_use]
fn bars(percentages: &[u32]) -> String {
    percentages.into_iter().map(|&percent| {
        let color = percent_col(percent);
        let bar = percent_bar(percent);
        format!("{color}{bar}{RESET}")
    }).collect()
}

fn max_freq(cpu_id: usize) -> std::io::Result<u32> {
    let path = format!("/sys/devices/system/cpu/cpu{cpu_id}/cpufreq/cpuinfo_max_freq");
    let contents = std::fs::read_to_string(path)?;
    Ok(contents.trim().parse::<u32>().unwrap_or(0) / 1000) // MHz
}

fn net_filter(net: (&String, &NetworkData)) -> bool {
    let (name, data) = net;
    !(name.contains("veth") || name == "lo" || name.starts_with("br-") || data.total_received() == 0 && data.total_transmitted() == 0)
}

// TODO: DISK-IO, CPU FANS
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let refresh_kind = RefreshKind::everything().without_processes();
    let mut sys = System::new_with_specifics(refresh_kind);
    let mut components = Components::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut nets = Networks::new_with_refreshed_list();
    let nvml = Nvml::init()?;
    let mobo = Motherboard::new().ok_or("No motherboard")?;

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"info".to_string()) {
        // CPU INFO
        let cpus = sys.cpus();
        let brand = cpus[0].brand().replace("(R)", "").replace("(TM)", "").replace("Intel ", "").replace("Core ", "");
        println!("{SKY}CPU{RESET} {brand} {BLUE}x{} Cores{RESET}", cpus.len());

        // GPU INFO
        let num_gpus = nvml.device_count()?;
        for i in 0..num_gpus {
            let gpu = nvml.device_by_index(i)?;
            let gpu_name = gpu.name()?.replace("NVIDIA ", "").replace("GeForce ", "");
            println!("{MAGENTA}GPU{RESET} {gpu_name}", );
            let mem_max_clk = gpu.max_clock_info(Clock::Memory)?;
            let tab = format!("{DIM}{MAGENTA}├─{RESET}");
            println!("{tab} VRAM {GREEN}{}GB{RESET} {BLUE}{mem_max_clk}MHz{RESET}", gpu.memory_info()?.total as f64 / (1u64 << 30u64) as f64);
            let gfx_max_clk = gpu.max_clock_info(Clock::Graphics)?; 
            let sm_max_clk = gpu.max_clock_info(Clock::SM)?;
            let vid_max_clk = gpu.max_clock_info(Clock::Video)?;
            println!("{tab} Clock {DIM}Gfx{RESET} {BLUE}{gfx_max_clk}MHz{RESET}  {DIM}SM{RESET} {BLUE}{sm_max_clk}MHz{RESET}  {DIM}Vid{RESET} {BLUE}{vid_max_clk}MHz{RESET}");
            println!("{tab} Cores {BLUE}{}{RESET}", gpu.num_cores()?);
            println!("{tab} Consumed {BLUE}{}MJ{RESET}", (gpu.total_energy_consumption()? as f32 / 1e9 * 100.0).round() / 100.0);
            println!("{tab} Driver {BLUE}{}{RESET}", nvml.sys_driver_version()?);
            println!("{tab} Perf {BLUE}{:?}{RESET} {DIM}(0-15, 0 = max){RESET}", gpu.performance_state()?.as_c());
            println!("{DIM}{MAGENTA}└─{RESET} CUDA {BLUE}{}{RESET}", nvml.sys_cuda_driver_version()?);
        }

        // MOTHERBOARD INFO
        println!("{RED}MOBO{RESET} {}", mobo.name().unwrap_or_default());
        
        // COMPONENT INFO
        let mut comp_names: Vec<String> = vec![];
        for comp in components.iter() {
            let name = comp.label().replace("Core ", "").replace("coretemp ", "core ");
            let is_core = name.strip_prefix("core ").map_or(false, |suffix| suffix.chars().all(|c| c.is_ascii_digit()));
            if name.contains("core Package") | is_core {
                continue;
            }
            let name = if let Some(rest) = name.strip_prefix("nvme Sensor ") {
                rest.chars().skip_while(|c| c.is_ascii_digit() || c.is_whitespace()).collect()
            } else if let Some(rest) = name.strip_prefix("nvme Composite ") {
                rest.to_string()
            } else {
                name
            };
            let mut name = name.replace("SSD ", "").replace(" temp1", "").replace("acpitz", "Motherboard").replace("spd5118", "RAM");
            if name.contains("wifi") {
                name = "Wi-Fi".to_string();
            }
            if name == "Motherboard" {
                continue;
            }
            if !comp_names.iter().any(|existing| {
                existing.starts_with(&name) || name.starts_with(existing)
            }) {
                comp_names.push(format!("{name}"));
            }
        }
        comp_names.sort();
        for (i, name) in comp_names.iter().enumerate() {
            let mut tab = format!("{DIM}{RED}├─{RESET}");
            if i == comp_names.len() - 1 {
                tab = format!("{DIM}{RED}└─{RESET}");
            }
            println!("{tab} {name}");
        }

        println!("{CYAN}Networks{RESET} ");
        let mut net_iter = nets.iter().filter(|&net| net_filter(net)).peekable();
        while let Some((name, data)) = net_iter.next() {
            let ips = data.ip_networks().iter().map(|ip| format!("{}{}{RESET}]", if ip.prefix == 24 { "ipv4[\x1b[2m" } else { "ipv6[\x1b[2m" }, ip.addr)).collect::<Vec<String>>().join(", ");
            let mut tab = format!("{DIM}{CYAN}├─{RESET}");
            if net_iter.peek().is_none() {
                tab = format!("{DIM}{CYAN}└─{RESET}");
            }
            println!("{tab} {BLUE}{name}{RESET} {ips}{RESET} mac[{DIM}{}{RESET}]", &data.mac_address());
        }

        return Ok(());
    }

    print!("\x1b[?1049h");
    loop {
        // refresh screen
        print!("\x1b[H\x1b[2J\x1b[?25l");

        // REFRESH
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

        sys.refresh_specifics(RefreshKind::everything().without_processes());
        disks.refresh(true);
        nets.refresh(true);
        components.refresh(true);
        
        let mut out = String::new();

        let gpu = nvml.device_by_index(0)?;

        // GPU FANS
        let num_fans = gpu.num_fans().unwrap_or(1);
        let mut fan_str = String::new();
        for i in 0..num_fans {
            let fan_percent = gpu.fan_speed(i).unwrap_or(0);
            let fan_rpm = gpu.fan_speed_rpm(i).unwrap_or(0);
            fan_str += &format!("{}{fan_percent}%{RESET} {DIM}{fan_rpm:>4}rpm{RESET}", percent_col(fan_percent));
            if i != num_fans - 1 {
                fan_str += ", ";
            }
        }

        // COMPONENT TEMPS
        let mut core_temps: Vec<u32> = vec![];
        let mut comp_temps: Vec<String> = vec![];
        let mut cpu_temp = 0;
        let mut comps = components.iter_mut().collect::<Vec<_>>();
        comps.sort_by_key(|c| {
            if c.label().contains("Composite") {
                0
            } else {
                100000 - c.temperature().unwrap_or(0.0) as u32
            }
        });
        for comp in comps {
            let name = comp.label().replace("Core ", "").replace("coretemp ", "core ");
            let is_core = name.strip_prefix("core ").map_or(false, |suffix| suffix.chars().all(|c| c.is_ascii_digit()));
            let temp = comp.temperature().unwrap_or(0.0).round() as u32;
            if name.contains("core Package") {
                cpu_temp = temp;
                continue;
            }
            if is_core {
                core_temps.push(temp);
                continue;
            }
            let name = if let Some(rest) = name.strip_prefix("nvme Sensor ") {
                rest.chars().skip_while(|c| c.is_ascii_digit() || c.is_whitespace()).collect()
            } else if let Some(rest) = name.strip_prefix("nvme Composite ") {
                rest.to_string()
            } else {
                name
            };
            let mut name = name.replace("SSD ", "").replace(" temp1", "").replace("acpitz", "Motherboard").replace("spd5118", "RAM");
            if name.contains("wifi") {
                name = "Wi-Fi".to_string();
            }
            let comp_temp = format!("{BLUE}{name}{RESET} ;{}{temp}°C{RESET}", percent_col(temp));
            
            if let Some(existing_index) = comp_temps.iter().position(|existing| {
                let existing_name = existing.split(';').next().unwrap_or("");
                let existing_name = existing_name.replace(&format!("{BLUE}"), "").replace(&format!("{RESET}"), "");
                existing_name.starts_with(&name) || name.starts_with(&existing_name)
            }) {
                // Append temperature to existing entry
                let existing = &mut comp_temps[existing_index];
                let temp_part = format!(", {}{temp}°C{RESET}", percent_col(temp));
                *existing += &temp_part;
            } else {
                comp_temps.push(comp_temp);
            }
        }
        comp_temps.sort();

        // SYSTEM UTILIZATION
        let cpu_usage = sys.global_cpu_usage().round() as u32;
        let gpu_utilization = gpu.utilization_rates()?;
        let gpu_usage = gpu_utilization.gpu;
        let gpu_temp = gpu.temperature(TemperatureSensor::Gpu)?;
        let gpu_mem_percent = gpu_utilization.memory;
        let gpu_power_usage = gpu.power_usage()? / 1000;
        let gpu_max_power = gpu.power_management_limit()? / 1000;
        let gpu_power_usage_percent = (gpu_power_usage as f32 / gpu_max_power as f32 * 100.0).round() as u32;
        let usage_str = format!(" {GREEN}CPU{RESET}{}{cpu_usage:>3}%{RESET} ({}{cpu_temp}°C{RESET});{MAGENTA}GPU{RESET}{}{gpu_usage:>3}%{RESET} ({}{gpu_temp}°C{RESET}{}{gpu_power_usage:>4}W{RESET}{DIM}/{RESET}{}{gpu_max_power}W{RESET});{RED}VRAM{RESET} {}{gpu_mem_percent}%{RESET}", 
            percent_col(cpu_usage), percent_col(cpu_temp), percent_col(gpu_usage), percent_col(gpu_temp), percent_col(gpu_power_usage_percent), percent_col(gpu_power_usage_percent), percent_col(gpu_mem_percent));
        write!(out, "{}", sized_rows(&[usage_str], &["CPU %".len() + 12, "VRAM %".len() + 12, "VRAM %".len() + 12]))?;

        // MEMORY USAGES
        let ram = mem_bar(sys.used_memory(), sys.total_memory(), 14);
        let swap = mem_usage(sys.used_swap(), sys.total_swap());
        writeln!(out, " {RED}RAM{RESET} {ram}  {RED}SWP{RESET} {swap}")?;

        let gpu_mem_info = gpu.memory_info()?;
        let vram = mem_bar(gpu_mem_info.used, gpu_mem_info.total, 14);
        writeln!(out, "{RED}VRAM {RESET}{vram} ")?;

        // CORE USAGES
        let cpus = sys.cpus();
        let cores: Vec<u32> = cpus.iter().map(|cpu| cpu.cpu_usage() as u32).collect();
        let max_core = cores.iter().copied().max().unwrap_or(0);
        
        let max_core_freqs: Vec<u32> = (0..cpus.len()).map(|i| max_freq(i).unwrap_or(1)).collect();
        let core_freqs: Vec<u32> = cpus.iter().zip(max_core_freqs.iter()).map(|(cpu, max_freq)| (cpu.frequency() as f32 / *max_freq as f32 * 100.0).round().min(100.0) as u32).collect();
        
        // CORE FREQS
        let max_core_freq = core_freqs.iter().copied().max().unwrap_or(0);
        let min_core_freq_rating = max_core_freqs.iter().copied().min().unwrap_or(0);
        let max_core_freq_rating = max_core_freqs.iter().copied().max().unwrap_or(0);
        let single_core_freq_rating = min_core_freq_rating == max_core_freq_rating;
        let rating = format!("{min_core_freq_rating}{}MHz", if single_core_freq_rating { String::new() } else  { format!("-{max_core_freq_rating}") });
        
        // CORE TEMPS
        let max_core_temp = core_temps.iter().copied().max().unwrap_or(0);

        writeln!(out, "{BLUE}CORE{RESET} {}{:>w$} {max_core}%{RESET}", &bars(&cores), percent_col(max_core), w = 5)?;
        writeln!(out, "{BLUE}FREQ{RESET} {}{:>w$} {max_core_freq}%{RESET} {DIM}{rating}{RESET}", bars(&core_freqs), percent_col(max_core_freq), w = 5)?;
        writeln!(out, "{BLUE}TEMP{RESET} {}{:>w$} {max_core_temp}C{RESET}", bars(&core_temps), percent_col(max_core_temp), w = 5 + cores.len() - core_temps.len())?;

        // GPU CLOCK
        let gfx_clk = gpu.clock_info(Clock::Graphics).unwrap_or(0);
        let gfx_max_clk = gpu.max_clock_info(Clock::Graphics).unwrap_or(0);
        let mem_clk = gpu.clock_info(Clock::Memory).unwrap_or(0);
        let mem_max_clk = gpu.max_clock_info(Clock::Memory).unwrap_or(0);
        let sm_clk = gpu.clock_info(Clock::SM).unwrap_or(0);
        let sm_max_clk = gpu.max_clock_info(Clock::SM).unwrap_or(0);
        let vid_clk = gpu.clock_info(Clock::Video).unwrap_or(0);
        let vid_max_clk = gpu.max_clock_info(Clock::Video).unwrap_or(0);
        writeln!(out, "{BLUE}CLCK{RESET} {DIM}GFX{RESET}{}  {DIM}MEM{RESET}{}  {DIM}SM{RESET}{}  {DIM}VID{RESET}{}", 
            mhz(gfx_clk, gfx_max_clk), mhz(mem_clk, mem_max_clk), mhz(sm_clk, sm_max_clk), mhz(vid_clk, vid_max_clk))?;

        // GPU FANS
        writeln!(out, "{SKY}FANS{RESET} {fan_str}")?;

        // PCIE
        let rx = gpu.pcie_throughput(PcieUtilCounter::Receive)? / 1000; // MBps
        let tx = gpu.pcie_throughput(PcieUtilCounter::Send)? / 1000; // MBps
        let pcie_gen = gpu.max_pcie_link_gen()?;
        let pcie_width = gpu.max_pcie_link_width()?;
        // PCIe throughput per lane in MB/s (accounting for encoding overhead)
        let pcie_throughput_per_lane = match pcie_gen {
            1 => 250,   // PCIe 1.0: 2.5 GT/s * 0.8 (8b/10b encoding) / 8 bits = 250 MB/s
            2 => 500,   // PCIe 2.0: 5.0 GT/s * 0.8 / 8 = 500 MB/s  
            3 => 985,   // PCIe 3.0: 8.0 GT/s * 0.9846 (128b/130b encoding) / 8 = 985 MB/s
            4 => 1969,  // PCIe 4.0: 16.0 GT/s * 0.9846 / 8 = 1969 MB/s
            5 => 3938,  // PCIe 5.0: 32.0 GT/s * 0.9846 / 8 = 3938 MB/s
            _ => 1969,  // Default to PCIe 4.0 if unknown
        };
        let max_pcie_throughtput = pcie_throughput_per_lane * pcie_width;
        let max_pcie_throughtput_gb = (max_pcie_throughtput as f32 / 1000.0 * 10.0).round() / 10.0; // GB/s
        let rx_col = percent_col((rx as f32 / max_pcie_throughtput as f32 * 100.0).round() as u32);
        let tx_col = percent_col((tx as f32 / max_pcie_throughtput as f32 * 100.0).round() as u32);
        writeln!(out, "{SKY}PCIE{RESET} {GREEN}▼{RESET}{rx_col}{rx:>4}M{RESET}  {MAGENTA}▲{RESET}{tx_col}{tx:>4}M{RESET}   {DIM}{max_pcie_throughtput_gb}GB/s{RESET}", )?;

        // NETWORK
        let net_iter = nets.iter().filter(|&net| net_filter(net)).collect::<Vec<_>>();
        if let Some((name, data)) = net_iter.iter().max_by_key(|(_, data)| Reverse(data.total_transmitted() + data.total_received())) {
            let (rx, tx) = (data.received() / 1024, data.transmitted() / 1024);
            let (prx, ptx) = (data.packets_received(), data.packets_transmitted());
            writeln!(out, "{SKY}NETW{RESET} {GREEN}▼{RESET}{BLUE}{rx:>4}K{RESET}  {MAGENTA}▲{RESET}{BLUE}{tx:>4}K{RESET} {GREEN}{prx:>4}{RESET}/{MAGENTA}{ptx:<4} {CYAN}pkt/s{RESET}  {DIM}{name}{RESET}")?;
        }

        // DISKS
        let mut disk_infos = vec![];
        for disk in disks.iter_mut() {
            let total = disk.total_space();
            const MIN_DISK_SIZE: u64 = 8 * (1 << 30); // 8 GiB
            if total <= MIN_DISK_SIZE { continue; }
            let free = disk.available_space();
            let usage = disk.usage();
            let name = disk.name().to_str().and_then(|d| d.strip_prefix("/dev/")).unwrap_or_default();
            let rw = format!("{GREEN}{:>4}{RESET}/{MAGENTA}{:<4}{RESET}", format_size(usage.read_bytes), format_size(usage.written_bytes));
            let total_rw = format!("{GREEN}{}{RESET}/{MAGENTA}{}{RESET}", format_size(usage.total_read_bytes), format_size(usage.total_written_bytes));
            let usage = mem_usage(total - free, total);
            disk_infos.push(format!("{SKY}{name}{RESET};{usage};{rw};Tot {total_rw}"))  
        }
        write!(out, "{}", rows(&disk_infos))?;

        // EXTRA COMPONENTS
        if args.contains(&"extra".to_string()) {
            write!(out, "{}", rows(&comp_temps))?;
        }

        print!("{out}\x1b[?25h");
    }
    // print!("\x1b[?1049l");
}