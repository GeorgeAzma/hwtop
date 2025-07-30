use sysinfo::{Components, Disks, Motherboard, NetworkData, Networks, RefreshKind, System};
use nvml_wrapper::{enum_wrappers::device::{Clock, PcieUtilCounter, TemperatureSensor}, Nvml};
use std::{cmp::Reverse, collections::BTreeMap, fmt::Write};


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

// TODO: DISK-IO, CPU FANS, FIX REFRESH, FIX CURSOR
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let refresh_kind = RefreshKind::everything().without_processes();
    let mut sys = System::new_with_specifics(refresh_kind);
    let mut components = Components::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut nets = Networks::new_with_refreshed_list();
    let nvml = Nvml::init()?;
    let mobo = Motherboard::new().ok_or("No motherboard")?;

    let args: Vec<String> = std::env::args().collect();

    let use_ansi = !args.contains(&"plain".to_string());
    let (red, green, magenta, cyan, sky, blue, reset, dim) = if use_ansi {
        ("\x1b[31m",
        "\x1b[32m",
        "\x1b[35m",
        "\x1b[36m",
        "\x1b[96m",
        "\x1b[94m",
        "\x1b[0m",
        "\x1b[2m")
    } else {
        Default::default()
    };

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

    let percent_col = |percent: u32| -> &'static str {
        match percent {
            0..=12 => blue,
            13..=25 => blue,
            26..=37 => sky,
            38..=50 => sky,
            51..=62 => magenta,
            63..=75 => magenta,
            76..=87 => red,
            _ => red,
        }
    };

    let mem_usage = |used: u64, total: u64| -> String {
        let used_str = format_size(used);
        let total_str = format_size(total);
        let percent = ((used as f64 / total as f64) * 100.0).round() as u32;
        format!("{}{used_str}{reset}/{}{total_str}{reset}", percent_col(percent), percent_col(percent))
    };

    let mhz = |clock: u32, max_clock: u32| -> String {
        let ratio = clock as f32 / max_clock as f32;
        let ratio = ratio * ratio; // better colors
        let percent = (ratio * 100.0).round() as u32;
        let col = percent_col(percent);
        format!(" {col}{}{reset}", percent_bar(percent))
    };

    let mem_bar = |used: u64, total: u64, width: u32| -> String {
        let ratio = (used as f64 / total as f64).min(1.0);
        let full_blocks = (ratio * width as f64) as u32;
        let col = percent_col((ratio * 100.0).round() as u32);
        let bar = "█".repeat(full_blocks as usize);
        let usage = mem_usage(used, total);
        if full_blocks == width {
            return format!("[{col}{bar}{reset}] {usage}");
        }
        let remainder_percent = ((ratio * width as f64).fract() * 100.0).round() as u32;
        let remainder = percent_slider(remainder_percent);
        let empty = &" ".repeat((width - full_blocks - 1) as usize);
        format!("[{col}{bar}{remainder}{reset}{empty}] {usage}")
    };

    let bars = |percentages: &[u32]| -> String {
        percentages.into_iter().map(|&percent| {
            let color = percent_col(percent);
            let bar = percent_bar(percent);
            format!("{color}{bar}{reset}")
        }).collect()
    };

    fn max_freq(cpu_id: usize) -> std::io::Result<u32> {
        let path = format!("/sys/devices/system/cpu/cpu{cpu_id}/cpufreq/cpuinfo_max_freq");
        let contents = std::fs::read_to_string(path)?;
        Ok(contents.trim().parse::<u32>().unwrap_or(0) / 1000) // MHz
    }

    fn net_filter(net: (&String, &NetworkData)) -> bool {
        let (name, data) = net;
        !(name.contains("veth") || name == "lo" || name.starts_with("br-") || data.total_received() == 0 && data.total_transmitted() == 0)
    }

    fn get_comp_temps(components: &mut Components) -> BTreeMap<String, Vec<u32>> {
        let mut comp_temps: BTreeMap<String, Vec<u32>> = BTreeMap::new();
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
                comp_temps.insert("CPU".to_string(), vec![temp]);
                continue;
            }
            if is_core {
                comp_temps.entry("Core".to_string())
                    .and_modify(|temps: &mut Vec<u32>| temps.push(temp))
                    .or_insert_with(|| vec![temp]);
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
            comp_temps
                .entry(name.clone())
                .and_modify(|temps: &mut Vec<u32>| temps.push(temp))
                .or_insert_with(|| vec![temp]);
        }
        comp_temps
    }
    
    if args.contains(&"info".to_string()) {
        // CPU INFO
        let cpus = sys.cpus();
        let brand = cpus[0].brand().replace("(R)", "").replace("(TM)", "").replace("Intel ", "").replace("Core ", "");
        println!("{sky}CPU{reset} {brand} {blue}x{} Cores{reset}", cpus.len());

        // GPU INFO
        let num_gpus = nvml.device_count()?;
        for i in 0..num_gpus {
            let gpu = nvml.device_by_index(i)?;
            let gpu_name = gpu.name()?.replace("NVIDIA ", "").replace("GeForce ", "");
            println!("{magenta}GPU{reset} {gpu_name}", );
            let mem_max_clk = gpu.max_clock_info(Clock::Memory)?;
            let tab = format!("{dim}{magenta}├─{reset}");
            println!("{tab} VRAM {green}{}GB{reset} {blue}{mem_max_clk}MHz{reset}", gpu.memory_info()?.total as f64 / (1u64 << 30u64) as f64);
            let gfx_max_clk = gpu.max_clock_info(Clock::Graphics)?; 
            let sm_max_clk = gpu.max_clock_info(Clock::SM)?;
            let vid_max_clk = gpu.max_clock_info(Clock::Video)?;
            println!("{tab} Clock {dim}Gfx{reset} {blue}{gfx_max_clk}MHz{reset}  {dim}SM{reset} {blue}{sm_max_clk}MHz{reset}  {dim}Vid{reset} {blue}{vid_max_clk}MHz{reset}");
            println!("{tab} Cores {blue}{}{reset}", gpu.num_cores()?);
            println!("{tab} Consumed {blue}{}MJ{reset}", (gpu.total_energy_consumption()? as f32 / 1e9 * 100.0).round() / 100.0);
            println!("{tab} Driver {blue}{}{reset}", nvml.sys_driver_version()?);
            println!("{tab} Perf {blue}{:?}{reset} {dim}(0-15, 0 = max){reset}", gpu.performance_state()?.as_c());
            println!("{dim}{magenta}└─{reset} CUDA {blue}{}{reset}", nvml.sys_cuda_driver_version()?);
        }

        // MOTHERBOARD INFO
        println!("{red}MOBO{reset} {}", mobo.name().unwrap_or_default());
        
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
            let mut tab = format!("{dim}{red}├─{reset}");
            if i == comp_names.len() - 1 {
                tab = format!("{dim}{red}└─{reset}");
            }
            println!("{tab} {name}");
        }

        println!("{cyan}Networks{reset} ");
        let mut net_iter = nets.iter().filter(|&net| net_filter(net)).peekable();
        while let Some((name, data)) = net_iter.next() {
            let ips = data.ip_networks().iter().map(|ip| format!("{}{}{reset}]", if ip.prefix == 24 { format!("ipv4[{dim}") } else { format!("ipv6[{dim}") }, ip.addr)).collect::<Vec<String>>().join(", ");
            let mut tab = format!("{dim}{cyan}├─{reset}");
            if net_iter.peek().is_none() {
                tab = format!("{dim}{cyan}└─{reset}");
            }
            println!("{tab} {blue}{name}{reset} {ips}{reset} mac[{dim}{}{reset}]", &data.mac_address());
        }

        return Ok(());
    }
    if use_ansi {
        print!("\x1b[?1049h");
    }
    loop {
        // refresh screen
        if use_ansi {
            print!("\x1b[H\x1b[2J\x1b[?25l");
        }

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
            fan_str += &format!("{}{fan_percent}%{reset} {dim}{fan_rpm:>4}rpm{reset}", percent_col(fan_percent));
            if i != num_fans - 1 {
                fan_str += ", ";
            }
        }

        // COMPONENT TEMPS
        let mut comp_temps = get_comp_temps(&mut components);
        let cpu_temp = comp_temps.remove("CPU").map(|v| v[0]).unwrap_or(0);
        let core_temps = comp_temps.remove("Core").unwrap_or_default();
        
        // SYSTEM UTILIZATION
        let cpu_usage = sys.global_cpu_usage().round() as u32;
        let gpu_utilization = gpu.utilization_rates()?;
        let gpu_usage = gpu_utilization.gpu;
        let gpu_temp = gpu.temperature(TemperatureSensor::Gpu)?;
        let gpu_mem_percent = gpu_utilization.memory;
        let gpu_power_usage = gpu.power_usage()? / 1000;
        let gpu_max_power = gpu.power_management_limit()? / 1000;
        let gpu_power_usage_percent = (gpu_power_usage as f32 / gpu_max_power as f32 * 100.0).round() as u32;
        let cpu_usage_str = format!(" {green}CPU{reset}{}{cpu_usage:>3}%{reset}{}{cpu_temp:>4}°C{reset}", 
            percent_col(cpu_usage), percent_col(cpu_temp));
        let gpu_usage_str = format!(" {magenta}GPU{reset}{}{gpu_usage:>3}%{reset}{}{gpu_temp:>4}°C {reset}{}{gpu_power_usage:>3}W{reset}{dim}/{reset}{}{gpu_max_power}W{reset}", 
            percent_col(gpu_usage), percent_col(gpu_temp), percent_col(gpu_power_usage_percent), percent_col(gpu_power_usage_percent));
        writeln!(out, "{cpu_usage_str}\n{gpu_usage_str}")?;

        // MEMORY USAGES
        let ram = mem_bar(sys.used_memory(), sys.total_memory(), 14);
        let swap = mem_usage(sys.used_swap(), sys.total_swap());
        writeln!(out, " {red}RAM{reset} {ram}  {swap}")?;

        let gpu_mem_info = gpu.memory_info()?;
        let vram = mem_bar(gpu_mem_info.used, gpu_mem_info.total, 14);
        writeln!(out, "{red}VRAM {reset}{vram}     {}{gpu_mem_percent}%{reset}", percent_col(gpu_mem_percent))?;

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
        let max_core_freq_str = format!("{max_core_freq}%");

        // CORE TEMPS
        let max_core_temp = core_temps.iter().copied().max().unwrap_or(0);

        writeln!(out, "{blue}CORE{reset} {}{:>w$} {max_core}%{reset}", &bars(&cores), percent_col(max_core), w = 5)?;
        writeln!(out, "{blue}FREQ{reset} {}{:>w$} {max_core_freq_str:<5}{reset}{dim}{rating}{reset}", bars(&core_freqs), percent_col(max_core_freq), w = 5)?;
        writeln!(out, "{blue}TEMP{reset} {}{:>w$} {max_core_temp}C{reset}", bars(&core_temps), percent_col(max_core_temp), w = 5 + cores.len() - core_temps.len())?;

        // GPU CLOCK
        let gfx_clk = gpu.clock_info(Clock::Graphics).unwrap_or(0);
        let gfx_max_clk = gpu.max_clock_info(Clock::Graphics).unwrap_or(0);
        let mem_clk = gpu.clock_info(Clock::Memory).unwrap_or(0);
        let mem_max_clk = gpu.max_clock_info(Clock::Memory).unwrap_or(0);
        let sm_clk = gpu.clock_info(Clock::SM).unwrap_or(0);
        let sm_max_clk = gpu.max_clock_info(Clock::SM).unwrap_or(0);
        let vid_clk = gpu.clock_info(Clock::Video).unwrap_or(0);
        let vid_max_clk = gpu.max_clock_info(Clock::Video).unwrap_or(0);
        writeln!(out, "{blue}CLCK{reset} {dim}GFX{reset}{}  {dim}MEM{reset}{}  {dim}SM{reset}{}  {dim}VID{reset}{}", 
            mhz(gfx_clk, gfx_max_clk), mhz(mem_clk, mem_max_clk), mhz(sm_clk, sm_max_clk), mhz(vid_clk, vid_max_clk))?;

        // GPU FANS
        writeln!(out, "{sky}FANS{reset} {fan_str}")?;

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
        writeln!(out, "{sky}PCIE{reset} {green}▼{reset}{rx_col}{rx:>4}M{reset}  {magenta}▲{reset}{tx_col}{tx:>4}M{reset}   {dim}{max_pcie_throughtput_gb}GB/s{reset}", )?;

        // NETWORK
        let net_iter = nets.iter().filter(|&net| net_filter(net)).collect::<Vec<_>>();
        if let Some((name, data)) = net_iter.iter().max_by_key(|(_, data)| Reverse(data.total_transmitted() + data.total_received())) {
            let (rx, tx) = (data.received() / 1024, data.transmitted() / 1024);
            let (prx, ptx) = (data.packets_received(), data.packets_transmitted());
            writeln!(out, "{sky}NETW{reset} {green}▼{reset}{blue}{rx:>4}K{reset}  {magenta}▲{reset}{blue}{tx:>4}K{reset} {green}{prx:>4}{reset}/{magenta}{ptx:<4} {cyan}pkt/s{reset}  {dim}{name}{reset}")?;
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
            let rw = format!("{green}{:>4}{reset}/{magenta}{:<4}{reset}", format_size(usage.read_bytes), format_size(usage.written_bytes));
            let total_rw = format!("{green}{}{reset}/{magenta}{}{reset}", format_size(usage.total_read_bytes), format_size(usage.total_written_bytes));
            let usage = mem_usage(total - free, total);
            disk_infos.push(format!("{sky}{name}{reset};{usage};{rw};Tot {total_rw}"))  
        }
        write!(out, "{}", rows(&disk_infos))?;

        // EXTRA COMPONENTS
        if args.contains(&"extra".to_string()) {
            let mut comp_temps = comp_temps.into_iter().collect::<Vec<_>>();
            comp_temps.sort_unstable();
            let comp_temps: Vec<String> = comp_temps.into_iter().map(|(name, temps)| {
                let mut comp_temp = format!("{blue}{name}{reset} ;");
                let mut first = true;
                for temp in temps {
                    if !first {
                        comp_temp += ", ";
                    } else {
                        first = false;
                    }
                    comp_temp += &format!("{}{temp}°C{reset}", percent_col(temp));
                }
                comp_temp
            }).collect();
            writeln!(out, "{}", rows(&comp_temps))?;
        }
        if use_ansi {
            print!("{out}\x1b[?25h");
        } else {
            print!("{out}");
            break Ok(());
        }
    }
    // if use _ansi {
    //     print!("\x1b[?1049l");
    // }
}