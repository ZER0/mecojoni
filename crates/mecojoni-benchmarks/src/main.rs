use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use mecojoni_benchmarks::{
    WORKLOAD_VERSION, Workload, operation_contract, workload_limits, workloads,
};
use mecojoni_core::{GenerationRequest, compile_package};

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        DEALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(pointer, old, new_size) };
        if !next.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            DEALLOCATED_BYTES.fetch_add(old.size() as u64, Ordering::Relaxed);
        }
        next
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy)]
struct AllocationSnapshot {
    calls: u64,
    allocated: u64,
    deallocated: u64,
}

impl AllocationSnapshot {
    fn now() -> Self {
        Self {
            calls: ALLOCATION_CALLS.load(Ordering::Relaxed),
            allocated: ALLOCATED_BYTES.load(Ordering::Relaxed),
            deallocated: DEALLOCATED_BYTES.load(Ordering::Relaxed),
        }
    }

    fn since(self, before: Self) -> (u64, u64, i128) {
        let allocated = self.allocated.saturating_sub(before.allocated);
        let deallocated = self.deallocated.saturating_sub(before.deallocated);
        (
            self.calls.saturating_sub(before.calls),
            allocated,
            i128::from(allocated) - i128::from(deallocated),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Measurement {
    rules: usize,
    productions: usize,
    compile_ns: u128,
    compile_calls: u64,
    compile_bytes: u64,
    compile_live: i128,
    generation_ns: u128,
    generation_calls: u64,
    generation_bytes: u64,
    generation_live: i128,
    expansions: u64,
    sampler_words: u64,
    output_bytes: u64,
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--contract") {
        println!(
            "version|scenario|source_bytes|rules|productions|artifact_hash|expansions|sampler_words|text"
        );
        for workload in workloads() {
            let contract = operation_contract(&workload);
            println!(
                "{WORKLOAD_VERSION}|{}|{}|{}|{}|{:016x}|{}|{}|{}",
                workload.name,
                contract.source_bytes,
                contract.rules,
                contract.productions,
                contract.artifact_hash,
                contract.expansions,
                contract.sampler_words,
                contract.text
            );
        }
        return;
    }
    let samples = 5;
    println!(
        "version,scenario,class,samples,source_bytes,rules,productions,compile_ns_median,compile_alloc_calls_median,compile_alloc_bytes_median,compile_alloc_bytes_min,compile_alloc_bytes_max,compile_live_bytes_median,generations,generation_ns_median,generation_alloc_calls_median,generation_alloc_bytes_median,generation_alloc_bytes_min,generation_alloc_bytes_max,generation_live_bytes_median,expansions,sampler_words,output_bytes"
    );
    for workload in workloads() {
        let measurements = (0..samples).map(|_| measure(&workload)).collect::<Vec<_>>();
        assert!(measurements.windows(2).all(|pair| {
            pair[0].expansions == pair[1].expansions
                && pair[0].sampler_words == pair[1].sampler_words
                && pair[0].output_bytes == pair[1].output_bytes
        }));
        let first = measurements[0];
        println!(
            "{WORKLOAD_VERSION},{},{},{samples},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            workload.name,
            workload.class,
            workload.source.len(),
            first.rules,
            first.productions,
            median(&measurements, |sample| sample.compile_ns),
            median(&measurements, |sample| sample.compile_calls),
            median(&measurements, |sample| sample.compile_bytes),
            minimum(&measurements, |sample| sample.compile_bytes),
            maximum(&measurements, |sample| sample.compile_bytes),
            median(&measurements, |sample| sample.compile_live),
            workload.generations,
            median(&measurements, |sample| sample.generation_ns),
            median(&measurements, |sample| sample.generation_calls),
            median(&measurements, |sample| sample.generation_bytes),
            minimum(&measurements, |sample| sample.generation_bytes),
            maximum(&measurements, |sample| sample.generation_bytes),
            median(&measurements, |sample| sample.generation_live),
            first.expansions,
            first.sampler_words,
            first.output_bytes,
        );
    }
}

fn measure(workload: &Workload) -> Measurement {
    let package = workload.package();
    let before_compile = AllocationSnapshot::now();
    let compile_started = Instant::now();
    let grammar = compile_package(&package).expect("committed workload compiles");
    let compile_ns = compile_started.elapsed().as_nanos();
    let after_compile = AllocationSnapshot::now();
    let (compile_calls, compile_bytes, compile_live) = after_compile.since(before_compile);

    let before_generation = AllocationSnapshot::now();
    let generation_started = Instant::now();
    let mut expansions = 0_u64;
    let mut sampler_words = 0_u64;
    let mut output_bytes = 0_u64;
    for seed in 0..workload.generations {
        let result = grammar
            .generate_weighted(&GenerationRequest {
                entry: None,
                seed: u64::from(seed),
                limits: workload_limits(),
                data: &[],
                trace_bindings: false,
                trace_selections: false,
                trace_provenance: false,
            })
            .expect("committed workload generates");
        expansions = expansions.saturating_add(u64::from(result.expansions()));
        sampler_words = sampler_words.saturating_add(u64::from(result.sampler_words()));
        output_bytes = output_bytes.saturating_add(result.text().len() as u64);
    }
    let generation_ns = generation_started.elapsed().as_nanos();
    let after_generation = AllocationSnapshot::now();
    let (generation_calls, generation_bytes, generation_live) =
        after_generation.since(before_generation);
    Measurement {
        rules: grammar.rule_count(),
        productions: grammar.production_count(),
        compile_ns,
        compile_calls,
        compile_bytes,
        compile_live,
        generation_ns,
        generation_calls,
        generation_bytes,
        generation_live,
        expansions,
        sampler_words,
        output_bytes,
    }
}

fn median<T: Copy + Ord>(measurements: &[Measurement], select: impl Fn(&Measurement) -> T) -> T {
    let mut values = measurements.iter().map(select).collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn minimum<T: Copy + Ord>(measurements: &[Measurement], select: impl Fn(&Measurement) -> T) -> T {
    measurements
        .iter()
        .map(select)
        .min()
        .expect("nonempty samples")
}

fn maximum<T: Copy + Ord>(measurements: &[Measurement], select: impl Fn(&Measurement) -> T) -> T {
    measurements
        .iter()
        .map(select)
        .max()
        .expect("nonempty samples")
}
