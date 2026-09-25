//! Minimal safe wrapper over the Performance Data Helper (PDH) API. Counters are added by
//! their English path so they work on every Windows display language.

use windows::core::HSTRING;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
    PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA,
};

use crate::provider::BoxError;

/// Doubles, without PDH's 100 % cap (callers clamp where appropriate).
const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x0000_8000);

fn check(what: &str, rc: u32) -> Result<(), BoxError> {
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("{what}: PDH error 0x{rc:08X}").into())
    }
}

fn valid(status: u32) -> bool {
    status == PDH_CSTATUS_VALID_DATA || status == PDH_CSTATUS_NEW_DATA
}

pub struct Query(PDH_HQUERY);

// SAFETY: PDH query handles are not tied to the creating thread; each Query is owned and used
// by one provider at a time.
unsafe impl Send for Query {}

impl Query {
    pub fn open() -> Result<Self, BoxError> {
        let mut h = PDH_HQUERY::default();
        // SAFETY: `h` is a valid out-pointer; a null data source means live data.
        check("PdhOpenQuery", unsafe { PdhOpenQueryW(None, 0, &mut h) })?;
        Ok(Self(h))
    }

    /// Adds a counter such as `\Processor Information(_Total)\% Processor Utility`. Wildcard
    /// instances (`(*)`) are re-expanded on every collect.
    pub fn add(&self, path: &str) -> Result<Counter, BoxError> {
        let mut c = PDH_HCOUNTER::default();
        // SAFETY: valid query handle and out-pointer; the path outlives the call.
        let rc = unsafe { PdhAddEnglishCounterW(self.0, &HSTRING::from(path), 0, &mut c) };
        check(path, rc)?;
        Ok(Counter(c))
    }

    pub fn collect(&self) -> Result<(), BoxError> {
        // SAFETY: valid query handle.
        check("PdhCollectQueryData", unsafe {
            PdhCollectQueryData(self.0)
        })
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: the handle came from PdhOpenQueryW; closing it frees its counters too.
        unsafe {
            PdhCloseQuery(self.0);
        }
    }
}

/// A counter handle. Only valid while its [`Query`] is alive; providers keep both together.
pub struct Counter(PDH_HCOUNTER);

// SAFETY: see `Query`.
unsafe impl Send for Counter {}

impl Counter {
    /// The latest formatted value; `None` until a rate counter has two samples.
    pub fn value(&self) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: valid counter handle and out-pointer.
        let rc = unsafe { PdhGetFormattedCounterValue(self.0, FMT, None, &mut v) };
        // SAFETY: FMT requests the double member of the union; every bit pattern is an f64.
        let value = unsafe { v.Anonymous.doubleValue };
        (rc == 0 && valid(v.CStatus)).then_some(value)
    }

    /// Every instance of a wildcard counter as `(instance name, value)`.
    pub fn array(&self) -> Vec<(String, f64)> {
        let mut size = 0u32;
        let mut count = 0u32;
        // SAFETY: size query with no buffer.
        let rc = unsafe { PdhGetFormattedCounterArrayW(self.0, FMT, &mut size, &mut count, None) };
        if rc != PDH_MORE_DATA {
            return Vec::new();
        }
        // Instances can appear between the calls; retry with the new size a few times.
        for _ in 0..4 {
            // u64 storage keeps the item array 8-byte aligned.
            let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
            let mut len = u32::try_from(buf.len() * 8).unwrap_or(u32::MAX);
            let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            // SAFETY: `buf` holds `len` writable, aligned bytes.
            let rc = unsafe {
                PdhGetFormattedCounterArrayW(self.0, FMT, &mut len, &mut count, Some(items))
            };
            if rc == PDH_MORE_DATA {
                size = len;
                continue;
            }
            if rc != 0 {
                return Vec::new();
            }
            // SAFETY: on success PDH wrote `count` items at the start of `buf`, with name
            // strings stored later in the same buffer.
            let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
            return items
                .iter()
                .filter(|it| valid(it.FmtValue.CStatus))
                .filter_map(|it| {
                    // SAFETY: NUL-terminated string inside `buf`; double member per FMT.
                    let name = unsafe { it.szName.to_string() }.ok()?;
                    Some((name, unsafe { it.FmtValue.Anonymous.doubleValue }))
                })
                .collect();
        }
        Vec::new()
    }
}
