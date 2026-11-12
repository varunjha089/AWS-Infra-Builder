//! IPv4 CIDR arithmetic: parse, contain, overlap, usable addresses (AWS keeps 5 of every subnet) and the next free block.

use anyhow::{bail, Result};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cidr {
    /// the network address (host bits cleared)
    pub addr: u32,
    pub prefix: u8,
}

impl Cidr {
    pub fn new(addr: u32, prefix: u8) -> Cidr {
        Cidr { addr: addr & mask(prefix), prefix }
    }

    /// "10.0.1.0/24" (a host address such as 10.0.1.7/24 is accepted and normalised, so a typo shows up as a different network)
    pub fn parse(s: &str) -> Result<Cidr> {
        let s = s.trim();
        let Some((ip, p)) = s.split_once('/') else { bail!("{s}: write an address and a prefix length, such as 10.0.0.0/16") };
        let prefix: u8 = match p.trim().parse() {
            Ok(v) if v <= 32 => v,
            _ => bail!("{s}: the prefix length must be between 0 and 32"),
        };
        let parts: Vec<&str> = ip.trim().split('.').collect();
        if parts.len() != 4 {
            bail!("{s}: the address needs four numbers separated by dots");
        }
        let mut addr: u32 = 0;
        for part in parts {
            let v: u32 = match part.parse() {
                Ok(v) if v <= 255 => v,
                _ => bail!("{s}: each number of the address must be between 0 and 255"),
            };
            addr = (addr << 8) | v;
        }
        Ok(Cidr::new(addr, prefix))
    }

    pub fn first(&self) -> u32 {
        self.addr
    }
    pub fn last(&self) -> u32 {
        self.addr | !mask(self.prefix)
    }
    /// the number of addresses in the block
    pub fn size(&self) -> u64 {
        1u64 << (32 - self.prefix as u32)
    }
    /// addresses AWS lets instances use: the block minus the 5 it reserves (network, router, DNS, future use, broadcast)
    pub fn usable(&self) -> u64 {
        self.size().saturating_sub(5)
    }
    pub fn contains(&self, other: &Cidr) -> bool {
        other.prefix >= self.prefix && (other.addr & mask(self.prefix)) == self.addr
    }
    pub fn overlaps(&self, other: &Cidr) -> bool {
        self.contains(other) || other.contains(self)
    }
    /// private address space (RFC 1918): 10/8, 172.16/12, 192.168/16
    pub fn is_private(&self) -> bool {
        let p = [Cidr::new(0x0A00_0000, 8), Cidr::new(0xAC10_0000, 12), Cidr::new(0xC0A8_0000, 16)];
        p.iter().any(|c| c.contains(self))
    }
    pub fn ip_string(addr: u32) -> String {
        format!("{}.{}.{}.{}", addr >> 24, (addr >> 16) & 255, (addr >> 8) & 255, addr & 255)
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", Cidr::ip_string(self.addr), self.prefix)
    }
}

fn mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    }
}

/// the first block of `prefix` inside `within` that overlaps none of `taken` (aligned to its own size), if there is room
pub fn next_free(within: &Cidr, taken: &[Cidr], prefix: u8) -> Option<Cidr> {
    if prefix < within.prefix || prefix > 32 {
        return None;
    }
    let step = 1u64 << (32 - prefix as u32);
    let mut start = within.first() as u64;
    let end = within.last() as u64;
    while start <= end {
        let c = Cidr::new(start as u32, prefix);
        match taken.iter().find(|t| t.overlaps(&c)) {
            None => return Some(c),
            // skip past the block that is in the way
            Some(t) => start = (t.last() as u64 + 1).max(start + step),
        }
        // keep the alignment: the next candidate starts on a multiple of its size
        start = start.div_ceil(step) * step;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print() {
        let c = Cidr::parse("10.0.1.7/24").unwrap();
        assert_eq!(c.to_string(), "10.0.1.0/24");
        assert_eq!(c.size(), 256);
        assert_eq!(c.usable(), 251);
        assert_eq!(Cidr::parse("0.0.0.0/0").unwrap().size(), 1 << 32);
        assert!(Cidr::parse("10.0.0.0").is_err());
        assert!(Cidr::parse("10.0.0.0/33").is_err());
        assert!(Cidr::parse("10.0.256.0/24").is_err());
        assert!(Cidr::parse("10.0.0/24").is_err());
    }

    #[test]
    fn contain_overlap_private() {
        let vpc = Cidr::parse("10.0.0.0/16").unwrap();
        let a = Cidr::parse("10.0.0.0/20").unwrap();
        let b = Cidr::parse("10.0.16.0/20").unwrap();
        let c = Cidr::parse("10.0.8.0/24").unwrap();
        assert!(vpc.contains(&a) && vpc.contains(&b) && !a.contains(&vpc));
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&c) && c.overlaps(&a));
        assert!(!vpc.contains(&Cidr::parse("10.1.0.0/24").unwrap()));
        assert!(vpc.is_private());
        assert!(Cidr::parse("172.31.0.0/16").unwrap().is_private());
        assert!(Cidr::parse("192.168.5.0/24").unwrap().is_private());
        assert!(!Cidr::parse("172.32.0.0/16").unwrap().is_private());
        assert!(!Cidr::parse("8.8.8.0/24").unwrap().is_private());
    }

    #[test]
    fn next_free_blocks() {
        let vpc = Cidr::parse("10.0.0.0/16").unwrap();
        let a = Cidr::parse("10.0.0.0/20").unwrap();
        assert_eq!(next_free(&vpc, &[], 20), Some(a));
        assert_eq!(next_free(&vpc, &[a], 20).unwrap().to_string(), "10.0.16.0/20");
        // a small block in the way: the next /20 after it stays aligned
        let small = Cidr::parse("10.0.17.0/24").unwrap();
        assert_eq!(next_free(&vpc, &[a, small], 20).unwrap().to_string(), "10.0.32.0/20");
        // a /24 fits in the gap before the small block
        assert_eq!(next_free(&vpc, &[a, small], 24).unwrap().to_string(), "10.0.16.0/24");
        // full
        let all: Vec<Cidr> = (0..16).map(|i| Cidr::new(0x0A00_0000 + (i << 12), 20)).collect();
        assert_eq!(next_free(&vpc, &all, 20), None);
        assert_eq!(next_free(&vpc, &[], 8), None);
    }

    /// random blocks: next_free never returns a block that overlaps a taken one or leaves the VPC, and it is aligned
    #[test]
    fn next_free_random() {
        let mut seed: u64 = 12345;
        let mut rnd = move |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        for _ in 0..500 {
            let vp = 16 + rnd(8) as u8;
            let vpc = Cidr::new((10 << 24) | ((rnd(256) as u32) << 16), vp);
            let mut taken = vec![];
            for _ in 0..rnd(6) {
                let p = vp + 1 + rnd(8) as u8;
                let off = rnd(vpc.size()) as u32;
                taken.push(Cidr::new(vpc.first() + off, p.min(28)));
            }
            let p = vp + rnd(10) as u8;
            if let Some(c) = next_free(&vpc, &taken, p) {
                assert!(vpc.contains(&c), "{c} not in {vpc}");
                assert!(taken.iter().all(|t| !t.overlaps(&c)), "{c} overlaps {taken:?}");
                assert_eq!(c.addr as u64 % c.size(), 0);
                assert_eq!(c.prefix, p);
            }
        }
    }
}
