use sysinfo::{System, RefreshKind, Disks, Components};
use nvml_wrapper::{enum_wrappers::device::{Clock, PcieUtilCounter, TemperatureSensor}, Nvml};
use std::fmt::Write;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const SKY: &str = "\x1b[96m";
const BLUE: &str = "\x1b[94m";
const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";


#[must_use]
fn mem_usage(used: u64, total: u64) -> String {
    let used_gb = format!("{:.1}", used as f64 / (1u64 << 30u64) as f64);
    let total_gb = format!("{}", (total as f64 / (1u64 << 30u64) as f64 * 10.0).round() / 10.0);
    format!("[{}{used_gb}G{RESET}{DIM}|{RESET}{GREEN}{total_gb}G{RESET}]", percent_col((used as f64 / total as f64 * 100.0).round() as u32))
}

#[must_use]
fn mhz(clock: u32, max_clock: u32) -> String {
    let ratio = clock as f32 / max_clock as f32;
    let ratio = ratio * ratio; // better colors for clock
    let percent = (ratio * 100.0).round() as u32;
    let col = percent_col(percent);
    format!("[{col}{}{RESET}]", percent_bar(percent))
}

#[must_use]
fn sized_rows(rows: &[String], sizes: &[usize]) -> String {
    let cols = rows.iter().map(|r| r.split(";").collect::<Vec<_>>()).collect::<Vec<_>>();
    if cols.is_empty() {
        return Default::default();
    }
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
    Ok(contents.trim().parse::<u32>().unwrap() / 1000) // MHz
}

// TODO: NETWORK, DISK Rx/Tx, FANS
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sys = System::new_with_specifics(
        RefreshKind::everything().without_processes(),
    );
    let components = Components::new_with_refreshed_list();
    let nvml = Nvml::init()?;

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"info".to_string()) {
        // CPU INFO
        let cpus = sys.cpus();
        let brand = cpus[0].brand().replace("(R)", "").replace("(TM)", "").replace("Intel ", "").replace("Core ", "");
        println!("{SKY}CPU{RESET} {brand} | {BLUE}x{}{RESET} Cores", cpus.len());

        // GPU INFO
        let num_gpus = nvml.device_count()?;
        for i in 0..num_gpus {
            let gpu = nvml.device_by_index(i)?;
            let gpu_name = gpu.name()?.replace("NVIDIA ", "").replace("GeForce ", "");
            println!("{MAGENTA}GPU{RESET} {gpu_name}", );
            let mem_max_clk = gpu.max_clock_info(Clock::Memory)?;
            let tab = format!("{DIM}{MAGENTA}├─{RESET}");
            println!("{tab} VRAM {GREEN}{}GB{RESET} {DIM}{mem_max_clk}MHz{RESET}", gpu.memory_info()?.total as f64 / (1u64 << 30u64) as f64);
            let gfx_max_clk = gpu.max_clock_info(Clock::Graphics)?; 
            let sm_max_clk = gpu.max_clock_info(Clock::SM)?;
            let vid_max_clk = gpu.max_clock_info(Clock::Video)?;
            println!("{tab} Clocks {DIM}Gfx{RESET}[{BLUE}{gfx_max_clk}{RESET}] {DIM}SM{RESET}[{BLUE}{sm_max_clk}{RESET}] {DIM}Vid{RESET}[{BLUE}{vid_max_clk}{RESET}]");
            println!("{tab} Cores {DIM}{}{RESET}", gpu.num_cores()?);
            println!("{tab} Consumed {DIM}{}{RESET}MJ", (gpu.total_energy_consumption()? as f32 / 1e9 * 100.0).round() / 100.0);
            println!("{tab} Driver {DIM}{}{RESET}", nvml.sys_driver_version()?);
            println!("{tab} Perf {DIM}{:?}{RESET}", gpu.performance_state()?);
            println!("{DIM}{MAGENTA}└─{RESET} CUDA {DIM}{}{RESET}", nvml.sys_cuda_driver_version()?);
        }

        // DISK INFO
        let disks = Disks::new_with_refreshed_list();
        let disk_rows = disks.list().iter().filter_map(|disk| {
            let Some(name) = disk.name().to_str() else { return None; };
            let name = name.strip_prefix("/dev/").unwrap_or(name);
            let kind = disk.kind().to_string();
            let Some(mnt) = disk.mount_point().to_str() else { return None; };
            if kind == "Unknown" || name == "overlay" {
                return None;
            }
            let fs = disk.file_system().to_string_lossy();
            Some(format!("{BLUE}{kind}{RESET};{DIM}{name}{RESET};{mnt};{MAGENTA}{fs}{RESET}"))
        }).collect::<Vec<_>>();
        println!("{}", rows(&disk_rows));

        return Ok(());
    }

    print!("\x1b[?1049h");
    loop {
        // refresh screen
        print!("\x1b[H\x1b[2J\x1b[?25l");

        // REFRESH
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_specifics(RefreshKind::everything().without_processes());
        let mut out = String::new();

        let gpu = nvml.device_by_index(0)?;

        // GPU FANS
        let num_fans = gpu.num_fans().unwrap_or(1);
        let mut fan_str = String::new();
        for i in 0..num_fans {
            let fan_percent = gpu.fan_speed(i).unwrap_or(0);
            let fan_rpm = gpu.fan_speed_rpm(i).unwrap_or(0);
            fan_str += &format!("({}{fan_percent}%{RESET} {DIM}{fan_rpm:>4}rpm{RESET})", percent_col(fan_percent));
            if i != num_fans - 1 {
                fan_str += ", ";
            }
        }

        // COMPONENT TEMPS
        let mut core_temps = vec![];
        let mut comp_temps = vec![];
        let mut cpu_temp = 0;
        for comp in components.list() {
            let name = comp.label().replace("Core ", "").replace("coretemp ", "core ");
            let is_core = name.strip_prefix("core ").map_or(false, |suffix| suffix.chars().all(|c| c.is_ascii_digit()));
            let temp = comp.temperature().unwrap_or(0.0).round() as u32;
            let _max = comp.max().unwrap_or(0.0).round() as u32;
            let crit = comp.critical().unwrap_or(0.0).round() as u32;
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
            let name = name.replace("SSD ", "").replace(" temp1", "");
            let crit = if crit == 0 { String::new() } else { format!("{DIM}/{RESET}{}{crit}°C{RESET}", percent_col(crit)) };
            let nvme_temp = format!("{BLUE}{name}{RESET} ;{}{temp}°C{RESET}{crit}", percent_col(temp));
            comp_temps.push(nvme_temp);
        }

        // SYSTEM UTILIZATION
        let cpu_usage = sys.global_cpu_usage().round() as u32;
        let gpu_utilization = gpu.utilization_rates()?;
        let gpu_usage = gpu_utilization.gpu;
        let gpu_temp = gpu.temperature(TemperatureSensor::Gpu)?;
        let gpu_mem_percent = gpu_utilization.memory;
        let gpu_power_usage = gpu.power_usage()? / 1000;
        let gpu_max_power = gpu.power_management_limit()? / 1000;
        let gpu_power_usage_percent = (gpu_power_usage as f32 / gpu_max_power as f32 * 100.0).round() as u32;
        let usage_str = format!("{SKY}CPU{RESET}[{}{cpu_usage}%{RESET}, {}{cpu_temp}°C{RESET}];{RED}GPU{RESET}[{}{gpu_usage}%{RESET}, {}{gpu_temp}°C{RESET}, {}{gpu_power_usage}W{RESET}{DIM}/{RESET}{}{gpu_max_power}W{RESET}];{MAGENTA}VRAM{RESET}[{}{gpu_mem_percent}%{RESET}]", 
            percent_col(cpu_usage), percent_col(cpu_temp), percent_col(gpu_usage), percent_col(gpu_temp), percent_col(gpu_power_usage_percent), percent_col(gpu_power_usage_percent), percent_col(gpu_mem_percent));
        write!(out, "{}", sized_rows(&[usage_str], &["CPU %".len() + 12, "VRAM %".len() + 12, "VRAM %".len() + 12]))?;
        
        // MEMORY USAGES
        let ram = mem_usage(sys.available_memory(), sys.total_memory());
        let ram = format!("{CYAN}RAM{RESET}{ram}");
        let gpu_mem_info = gpu.memory_info()?;
        let vram = mem_usage(gpu_mem_info.used, gpu_mem_info.total);
        let vram = format!("{CYAN}VRAM{RESET}{vram}");
        let swap = mem_usage(sys.used_swap(), sys.total_swap());
        let swap = format!("{CYAN}SWP{RESET}{swap}"); 
        writeln!(out, "{ram} {vram} {swap}")?;
        
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
        writeln!(out, "Rx {rx_col}{rx}M{RESET} Tx {tx_col}{tx}M{RESET} {DIM}{max_pcie_throughtput_gb}GB/s{RESET}", )?;
            
        // GPU FANS
        writeln!(out, "{CYAN}Fan{RESET}[{fan_str}]")?;

        // CPU CORE USAGES
        write!(out, "{BLUE}CORE{RESET} ")?;
        let cpus = sys.cpus();
        let cores: Vec<u32> = cpus.iter().map(|cpu| cpu.cpu_usage() as u32).collect();
        write!(out, "{}", &bars(&cores))?;
        let max_core = cores.iter().copied().max().unwrap_or(0);
        write!(out, " {}{max_core}%{RESET}\n", percent_col(max_core))?;
        
        let max_core_freqs: Vec<u32> = (0..cpus.len()).map(|i| max_freq(i).unwrap_or(1)).collect();
        let core_freqs: Vec<u32> = cpus.iter().zip(max_core_freqs.iter()).map(|(cpu, max_freq)| (cpu.frequency() as f32 / *max_freq as f32 * 100.0).round() as u32).collect();
        write!(out, "{MAGENTA}FREQ{RESET} ")?;
        write!(out, "{}", bars(&core_freqs))?;
        
        // CPU CORE FREQS
        let max_core_freq = core_freqs.iter().copied().max().unwrap_or(0);
        let min_core_freq_rating = max_core_freqs.iter().copied().min().unwrap_or(0);
        let max_core_freq_rating = max_core_freqs.iter().copied().max().unwrap_or(0);
        let single_core_freq_rating = min_core_freq_rating == max_core_freq_rating;
        let rating = format!("{min_core_freq_rating}{}MHz", if single_core_freq_rating { String::new() } else  { format!("-{max_core_freq_rating}") });
        write!(out, " {}{max_core_freq}%{RESET} {DIM}{rating}{RESET}\n", percent_col(max_core_freq))?;

        let max_core_temp = core_temps.iter().copied().max().unwrap_or(0);
        write!(out, "{RED}TEMP{RESET} ")?;
        writeln!(out, "{}       {}{max_core_temp}°C{RESET}", bars(&core_temps), percent_col(max_core_temp))?;

        let gfx_clk = gpu.clock_info(Clock::Graphics).unwrap_or(0);
        let gfx_max_clk = gpu.max_clock_info(Clock::Graphics).unwrap_or(0);
        let mem_clk = gpu.clock_info(Clock::Memory).unwrap_or(0);
        let mem_max_clk = gpu.max_clock_info(Clock::Memory).unwrap_or(0);
        let sm_clk = gpu.clock_info(Clock::SM).unwrap_or(0);
        let sm_max_clk = gpu.max_clock_info(Clock::SM).unwrap_or(0);
        let vid_clk = gpu.clock_info(Clock::Video).unwrap_or(0);
        let vid_max_clk = gpu.max_clock_info(Clock::Video).unwrap_or(0);
        writeln!(out, "{GREEN}CLK{RESET} {DIM}GFX{RESET}{} {DIM}Mem{RESET}{} {DIM}SM{RESET}{} {DIM}Vid{RESET}{}", 
            mhz(gfx_clk, gfx_max_clk), mhz(mem_clk, mem_max_clk), mhz(sm_clk, sm_max_clk), mhz(vid_clk, vid_max_clk))?;

        if args.contains(&"extra".to_string()) {
            write!(out, "{}", rows(&comp_temps))?;
        }

        print!("{out}");
    }
    // print!("\x1b[?1049l");
}