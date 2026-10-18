//! `-pin`: each worker stays on one CPU, chosen from the CPUs the process
//! may run on (so `numactl` and `taskset` still choose the set). The CPUs
//! are grouped by node (or by last-level cache), each group ordered one
//! hardware thread of every core before the second; `compact` fills one
//! group before the next, `spread` and `llc` deal workers to the nodes or
//! caches in turn. Linux only; elsewhere `-pin` changes nothing.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Compact,
    Spread,
    Llc,
}

impl Order {
    pub fn parse(s: &str) -> Option<Order> {
        match s {
            "compact" => Some(Order::Compact),
            "spread" => Some(Order::Spread),
            "llc" => Some(Order::Llc),
            _ => None,
        }
    }
}

/// The CPUs to pin workers to, in order: worker w gets entry w modulo the
/// length. Empty when the topology cannot be read.
#[cfg(target_os = "linux")]
pub fn cpu_order(order: Order) -> Vec<usize> {
    let allowed = allowed_cpus();
    let read = |p: String| std::fs::read_to_string(p).ok().map(|s| parse_list(&s));
    let rank = |c: usize| {
        read(format!("/sys/devices/system/cpu/cpu{c}/topology/thread_siblings_list"))
            .and_then(|l| l.iter().position(|&x| x == c))
            .unwrap_or(0)
    };
    // each group: its first CPU (for ordering the groups) and its CPUs
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    if order == Order::Llc {
        for &c in &allowed {
            let shared = read(format!("/sys/devices/system/cpu/cpu{c}/cache/index3/shared_cpu_list")).unwrap_or_else(|| vec![c]);
            let first = shared.first().copied().unwrap_or(c);
            match groups.iter_mut().find(|g| g.0 == first) {
                Some(g) => g.1.push(c),
                None => groups.push((first, vec![c])),
            }
        }
    } else if let Ok(rd) = std::fs::read_dir("/sys/devices/system/node") {
        for e in rd.flatten() {
            let Some(n) = e.file_name().to_str().and_then(|s| s.strip_prefix("node")?.parse::<usize>().ok()) else { continue };
            let Some(cpus) = read(e.path().join("cpulist").display().to_string()) else { continue };
            groups.push((n, cpus.into_iter().filter(|c| allowed.contains(c)).collect()));
        }
    }
    if groups.is_empty() {
        groups.push((0, allowed.clone()));
    }
    groups.sort();
    let lists: Vec<Vec<usize>> = groups
        .into_iter()
        .map(|(_, cpus)| {
            let mut v: Vec<(usize, usize)> = cpus.into_iter().map(|c| (rank(c), c)).collect();
            v.sort();
            v.into_iter().map(|(_, c)| c).collect()
        })
        .collect();
    if order == Order::Compact {
        return lists.concat();
    }
    let mut out = Vec::new();
    for i in 0..lists.iter().map(Vec::len).max().unwrap_or(0) {
        out.extend(lists.iter().filter_map(|l| l.get(i)));
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn cpu_order(_order: Order) -> Vec<usize> {
    Vec::new()
}

/// "0-3,8,10-11" as a list of CPUs.
#[cfg(target_os = "linux")]
fn parse_list(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
            out.extend(a..=b);
        }
    }
    out
}

#[cfg(target_os = "linux")]
const MASK_WORDS: usize = 16;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn sched_getaffinity(pid: i32, size: usize, mask: *mut u64) -> i32;
    fn sched_setaffinity(pid: i32, size: usize, mask: *const u64) -> i32;
}

#[cfg(target_os = "linux")]
fn allowed_cpus() -> Vec<usize> {
    let mut mask = [0u64; MASK_WORDS];
    // SAFETY: the kernel writes at most `size` bytes into the mask
    if unsafe { sched_getaffinity(0, std::mem::size_of_val(&mask), mask.as_mut_ptr()) } != 0 {
        return Vec::new();
    }
    (0..MASK_WORDS * 64).filter(|&c| mask[c / 64] & (1 << (c % 64)) != 0).collect()
}

/// Keep the calling thread on `cpu`.
#[cfg(target_os = "linux")]
pub fn pin(cpu: usize) {
    if cpu >= MASK_WORDS * 64 {
        return;
    }
    let mut mask = [0u64; MASK_WORDS];
    mask[cpu / 64] = 1 << (cpu % 64);
    // SAFETY: a plain libc call on a properly sized mask; a failure leaves
    // the thread where it was
    unsafe {
        sched_setaffinity(0, std::mem::size_of_val(&mask), mask.as_ptr());
    }
}

#[cfg(not(target_os = "linux"))]
pub fn pin(_cpu: usize) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn parse_list_reads_sysfs_ranges() {
        assert_eq!(super::parse_list("0-2,8,10-11\n"), vec![0, 1, 2, 8, 10, 11]);
        assert_eq!(super::parse_list("\n"), Vec::<usize>::new());
    }
}
