//! Minimal reader for the Mono runtime embedded in Unity games (x64, the
//! `mono-2.0-bdwgc.dll` of Unity 2021.3–2022.3), walking its structures from
//! outside the process: root domain → assemblies → class cache → fields.
//!
//! Structure offsets follow Mono's `domain-internals.h`/`class-internals.h`
//! as built by Unity; they match the community UnitySpy tables and were
//! checked against Hearthstone's runtime.

use std::io;

use anyhow::{bail, Context, Result};

mod off {
    pub const DOMAIN_ASSEMBLIES: u64 = 0xa0;
    pub const ASSEMBLY_NAME: u64 = 0x10;
    pub const ASSEMBLY_IMAGE: u64 = 0x60;
    pub const IMAGE_CLASS_CACHE: u64 = 0x4d0;
    pub const HASH_TABLE_SIZE: u64 = 0x18;
    pub const HASH_TABLE_TABLE: u64 = 0x20;
    pub const CLASS_PARENT: u64 = 0x30;
    pub const CLASS_NAME: u64 = 0x48;
    pub const CLASS_NAMESPACE: u64 = 0x50;
    pub const CLASS_VTABLE_SIZE: u64 = 0x5c;
    pub const CLASS_FIELDS: u64 = 0x98;
    pub const CLASS_RUNTIME_INFO: u64 = 0xd0;
    pub const CLASS_FIELD_COUNT: u64 = 0x100;
    pub const CLASS_NEXT_CACHE: u64 = 0x108;
    pub const FIELD_SIZE: u64 = 0x20;
    pub const FIELD_NAME: u64 = 0x8;
    pub const FIELD_OFFSET: u64 = 0x18;
    pub const TYPE_ATTRS: u64 = 0x8;
    pub const RUNTIME_INFO_VTABLES: u64 = 0x8;
    pub const VTABLE_METHODS: u64 = 0x48;
    pub const ARRAY_DATA: u64 = 0x20;
}

/// `FIELD_ATTRIBUTE_STATIC` in a `MonoType`'s attributes.
const ATTR_STATIC: u32 = 0x10;

/// Sanity bounds so corrupt pointers cannot send a walk into a long loop.
const MAX_LIST_NODES: usize = 2000;
const MAX_CLASS_CACHE: u32 = 1 << 20;
const MAX_FIELDS: u32 = 4096;
const MAX_INT_LIST: i32 = 256;

pub trait Memory {
    fn read_into(&self, addr: u64, buf: &mut [u8]) -> io::Result<()>;

    fn bytes(&self, addr: u64, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; n];
        self.read_into(addr, &mut buf).with_context(|| format!("reading {n} bytes at {addr:#x}"))?;
        Ok(buf)
    }

    fn u64(&self, addr: u64) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(addr, 8)?.try_into().unwrap()))
    }

    fn u32(&self, addr: u64) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(addr, 4)?.try_into().unwrap()))
    }

    fn i32(&self, addr: u64) -> Result<i32> {
        Ok(self.u32(addr)? as i32)
    }

    fn u16(&self, addr: u64) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(addr, 2)?.try_into().unwrap()))
    }

    /// NUL-terminated string of at most 256 bytes. Reads never cross a page
    /// boundary needlessly, so strings at the end of a mapping still work.
    fn cstr(&self, addr: u64) -> Result<String> {
        let mut out = Vec::new();
        let mut at = addr;
        while out.len() < 256 {
            let n = (64 - (at % 64)) as usize;
            let buf = self.bytes(at, n)?;
            if let Some(end) = buf.iter().position(|&b| b == 0) {
                out.extend_from_slice(&buf[..end]);
                break;
            }
            out.extend_from_slice(&buf);
            at += n as u64;
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub offset: u64,
    pub is_static: bool,
}

pub struct Mono<M> {
    pub mem: M,
    root_domain: u64,
}

/// Address of an exported function of a PE image mapped at `base`.
fn pe_export<M: Memory>(mem: &M, base: u64, name: &str) -> Result<u64> {
    if mem.bytes(base, 2)? != b"MZ" {
        bail!("no PE image at {base:#x}");
    }
    let pe = base + mem.u32(base + 0x3c)? as u64;
    if mem.bytes(pe, 4)? != b"PE\0\0" {
        bail!("bad PE signature");
    }
    let opt = pe + 24;
    if mem.u16(opt)? != 0x20b {
        bail!("not a 64-bit image");
    }
    let exports = base + mem.u32(opt + 112)? as u64;
    let count = mem.u32(exports + 24)?.min(65536);
    let functions = base + mem.u32(exports + 28)? as u64;
    let names = base + mem.u32(exports + 32)? as u64;
    let ordinals = base + mem.u32(exports + 36)? as u64;
    for i in 0..count as u64 {
        if mem.cstr(base + mem.u32(names + 4 * i)? as u64)? == name {
            let ordinal = mem.u16(ordinals + 2 * i)? as u64;
            return Ok(base + mem.u32(functions + 4 * ordinal)? as u64);
        }
    }
    bail!("{name} is not exported")
}

impl<M: Memory> Mono<M> {
    /// Finds the root domain through `mono_get_root_domain`, whose body is
    /// `mov rax, [rip+disp32]; ret`.
    pub fn attach(mem: M, mono_base: u64) -> Result<Mono<M>> {
        let func = pe_export(&mem, mono_base, "mono_get_root_domain")?;
        let code = mem.bytes(func, 8)?;
        if code[..3] != [0x48, 0x8b, 0x05] || code[7] != 0xc3 {
            bail!("unexpected mono_get_root_domain code {code:02x?}");
        }
        let disp = i32::from_le_bytes(code[3..7].try_into().unwrap()) as i64;
        let root_domain = mem.u64((func as i64 + 7 + disp) as u64)?;
        if root_domain == 0 {
            bail!("Mono is not initialised yet");
        }
        Ok(Mono { mem, root_domain })
    }

    /// `MonoImage*` of a loaded assembly.
    pub fn image(&self, assembly: &str) -> Result<u64> {
        let mut node = self.mem.u64(self.root_domain + off::DOMAIN_ASSEMBLIES)?;
        for _ in 0..MAX_LIST_NODES {
            if node == 0 {
                break;
            }
            let asm = self.mem.u64(node)?;
            if asm != 0 && self.mem.cstr(self.mem.u64(asm + off::ASSEMBLY_NAME)?)? == assembly {
                return self.mem.u64(asm + off::ASSEMBLY_IMAGE);
            }
            node = self.mem.u64(node + 8)?;
        }
        bail!("assembly {assembly} not loaded")
    }

    /// Looks a class up in an image's class cache.
    pub fn class(&self, image: u64, namespace: &str, name: &str) -> Result<u64> {
        let cache = image + off::IMAGE_CLASS_CACHE;
        let size = self.mem.u32(cache + off::HASH_TABLE_SIZE)?;
        if size == 0 || size > MAX_CLASS_CACHE {
            bail!("implausible class cache size {size}");
        }
        let table = self.mem.u64(cache + off::HASH_TABLE_TABLE)?;
        let slots = self.mem.bytes(table, size as usize * 8)?;
        for slot in slots.as_chunks::<8>().0 {
            let mut class = u64::from_le_bytes(*slot);
            for _ in 0..MAX_LIST_NODES {
                if class == 0 {
                    break;
                }
                if self.mem.cstr(self.mem.u64(class + off::CLASS_NAME)?)? == name
                    && self.mem.cstr(self.mem.u64(class + off::CLASS_NAMESPACE)?)? == namespace
                {
                    return Ok(class);
                }
                class = self.mem.u64(class + off::CLASS_NEXT_CACHE)?;
            }
        }
        bail!("class {namespace}.{name} not found")
    }

    /// A field of `class` or of one of its base classes.
    pub fn field(&self, class: u64, name: &str) -> Result<Field> {
        let mut k = class;
        for _ in 0..64 {
            if k == 0 {
                break;
            }
            let count = self.mem.u32(k + off::CLASS_FIELD_COUNT)?;
            let fields = self.mem.u64(k + off::CLASS_FIELDS)?;
            if fields != 0 && count <= MAX_FIELDS {
                for i in 0..count as u64 {
                    let f = fields + i * off::FIELD_SIZE;
                    if self.mem.cstr(self.mem.u64(f + off::FIELD_NAME)?)? == name {
                        let ty = self.mem.u64(f)?;
                        let attrs = self.mem.u32(ty + off::TYPE_ATTRS)?;
                        return Ok(Field {
                            offset: self.mem.i32(f + off::FIELD_OFFSET)?.max(0) as u64,
                            is_static: attrs & ATTR_STATIC != 0,
                        });
                    }
                }
            }
            k = self.mem.u64(k + off::CLASS_PARENT)?;
        }
        bail!("field {name} not found")
    }

    /// Start of a class's static field storage (root domain), or 0 when the
    /// class has not been initialised yet.
    pub fn static_data(&self, class: u64) -> Result<u64> {
        let info = self.mem.u64(class + off::CLASS_RUNTIME_INFO)?;
        if info == 0 {
            return Ok(0);
        }
        let vtable = self.mem.u64(info + off::RUNTIME_INFO_VTABLES)?;
        if vtable == 0 {
            return Ok(0);
        }
        let methods = self.mem.i32(class + off::CLASS_VTABLE_SIZE)?.max(0) as u64;
        self.mem.u64(vtable + off::VTABLE_METHODS + 8 * methods)
    }

    /// Class of a managed object (`MonoObject.vtable->klass`).
    pub fn class_of(&self, object: u64) -> Result<u64> {
        self.mem.u64(self.mem.u64(object)?)
    }

    /// Elements of a `List<T>` whose `T` is a 32-bit integer or enum.
    pub fn int_list(&self, list: u64) -> Result<Vec<i32>> {
        let class = self.class_of(list)?;
        let items = self.mem.u64(list + self.field(class, "_items")?.offset)?;
        let size = self.mem.i32(list + self.field(class, "_size")?.offset)?;
        if !(0..=MAX_INT_LIST).contains(&size) {
            bail!("implausible list size {size}");
        }
        if size == 0 || items == 0 {
            return Ok(Vec::new());
        }
        let raw = self.mem.bytes(items + off::ARRAY_DATA, size as usize * 4)?;
        Ok(raw.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect())
    }
}

#[cfg(test)]
pub mod fake {
    //! An in-memory address space for tests.
    use std::collections::BTreeMap;
    use std::io;

    use super::Memory;

    #[derive(Default)]
    pub struct FakeMemory {
        regions: BTreeMap<u64, Vec<u8>>,
        next: u64,
    }

    impl FakeMemory {
        pub fn new() -> FakeMemory {
            FakeMemory { regions: BTreeMap::new(), next: 0x10_0000 }
        }

        /// Allocates zeroed whole pages and returns their address.
        pub fn alloc(&mut self, size: usize) -> u64 {
            let addr = self.next;
            let pages = (size + 0xfff) & !0xfff;
            self.regions.insert(addr, vec![0; pages]);
            self.next += pages as u64 + 0x1000;
            addr
        }

        pub fn write(&mut self, addr: u64, data: &[u8]) {
            let (&base, region) = self.regions.range_mut(..=addr).next_back().expect("mapped");
            let start = (addr - base) as usize;
            region[start..start + data.len()].copy_from_slice(data);
        }

        pub fn put_u64(&mut self, addr: u64, v: u64) {
            self.write(addr, &v.to_le_bytes());
        }

        pub fn put_u32(&mut self, addr: u64, v: u32) {
            self.write(addr, &v.to_le_bytes());
        }

        pub fn string(&mut self, s: &str) -> u64 {
            let a = self.alloc(s.len() + 1);
            self.write(a, s.as_bytes());
            a
        }
    }

    impl Memory for FakeMemory {
        fn read_into(&self, addr: u64, buf: &mut [u8]) -> io::Result<()> {
            let fault = || io::Error::new(io::ErrorKind::InvalidInput, format!("unmapped {addr:#x}"));
            let (&base, region) = self.regions.range(..=addr).next_back().ok_or_else(fault)?;
            let start = (addr - base) as usize;
            let src = region.get(start..start + buf.len()).ok_or_else(fault)?;
            buf.copy_from_slice(src);
            Ok(())
        }
    }
}
