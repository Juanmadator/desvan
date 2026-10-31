//! Utilidades pequeñas: tiempos de Windows, formato de tamaños.

/// FILETIME (intervalos de 100 ns desde 1601) a segundos Unix.
pub fn filetime_to_unix(ft: u64) -> i64 {
    if ft == 0 {
        return 0;
    }
    (ft / 10_000_000) as i64 - 11_644_473_600
}

/// "1,5 GB" (base 1024, coma decimal).
pub fn human(bytes: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if v >= 100.0 { format!("{v:.0}") } else if v >= 10.0 { format!("{v:.1}") } else { format!("{v:.2}") };
    format!("{} {}", s.replace('.', ","), U[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn times_and_sizes() {
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1,50 KB");
        assert_eq!(human(10 * 1024 * 1024 * 1024), "10,0 GB");
    }
}
