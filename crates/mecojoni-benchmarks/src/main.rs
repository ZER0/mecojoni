use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use mecojoni_benchmarks::{
    STARTUP_PROFILE_VERSION, WORKLOAD_VERSION, Workload, harbor_startup_package,
    operation_contract, workload_limits, workloads,
};
use mecojoni_core::{
    ArtifactLimits, ArtifactOptions, DataBinding, GenerationRequest, Rational, Value,
    compile_package, compile_package_with_manifest, decode_artifact, encode_artifact,
};

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
    if std::env::args().nth(1).as_deref() == Some("--write-artifacts") {
        let directory = std::env::args()
            .nth(2)
            .expect("--write-artifacts requires a directory");
        write_workload_artifacts(std::path::Path::new(&directory));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--artifact") {
        measure_artifacts();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--artifact-startup") {
        measure_artifact_startup();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--startup") {
        measure_startup();
        return;
    }
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

fn write_workload_artifacts(directory: &std::path::Path) {
    std::fs::create_dir_all(directory).expect("create artifact directory");
    for workload in workloads() {
        let grammar = compile_package(&workload.package()).expect("workload compiles");
        let bytes =
            encode_artifact(&grammar, ArtifactOptions::default()).expect("workload encodes");
        let path = directory.join(format!("{}.mecob", workload.name));
        std::fs::write(&path, bytes).expect("write workload artifact");
        println!("{}", path.display());
    }
}

fn measure_artifacts() {
    let samples = 5;
    println!(
        "version,scenario,class,samples,source_bytes,artifact_bytes,encode_ns,rules,productions,load_ns_median,load_alloc_calls_median,load_alloc_bytes_median,load_live_bytes_median,generations,generation_ns_median,generation_alloc_calls_median,generation_alloc_bytes_median,generation_live_bytes_median,expansions,sampler_words,output_bytes"
    );
    for workload in workloads() {
        let source = compile_package(&workload.package()).expect("workload compiles");
        let encode_started = Instant::now();
        let bytes = encode_artifact(&source, ArtifactOptions::default()).expect("workload encodes");
        let encode_ns = encode_started.elapsed().as_nanos();
        let measurements = (0..samples)
            .map(|_| measure_decoded(&workload, &bytes))
            .collect::<Vec<_>>();
        let first = measurements[0];
        println!(
            "{WORKLOAD_VERSION},{},{},{samples},{},{},{encode_ns},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            workload.name,
            workload.class,
            workload.source.len(),
            bytes.len(),
            first.rules,
            first.productions,
            median(&measurements, |sample| sample.compile_ns),
            median(&measurements, |sample| sample.compile_calls),
            median(&measurements, |sample| sample.compile_bytes),
            median(&measurements, |sample| sample.compile_live),
            workload.generations,
            median(&measurements, |sample| sample.generation_ns),
            median(&measurements, |sample| sample.generation_calls),
            median(&measurements, |sample| sample.generation_bytes),
            median(&measurements, |sample| sample.generation_live),
            first.expansions,
            first.sampler_words,
            first.output_bytes,
        );
    }
}

fn measure_decoded(workload: &Workload, bytes: &[u8]) -> Measurement {
    let before_compile = AllocationSnapshot::now();
    let compile_started = Instant::now();
    let grammar = decode_artifact(bytes, ArtifactLimits::default()).expect("artifact decodes");
    let compile_ns = compile_started.elapsed().as_nanos();
    let after_compile = AllocationSnapshot::now();
    let (compile_calls, compile_bytes, compile_live) = after_compile.since(before_compile);
    measure_generation(
        workload,
        &grammar,
        compile_ns,
        compile_calls,
        compile_bytes,
        compile_live,
    )
}

fn measure_artifact_startup() {
    let samples = 5;
    let package = harbor_startup_package().expect("committed Harbor package loads");
    let source = compile_package_with_manifest(&package.input, &package.manifest)
        .expect("committed Harbor package compiles");
    let encode_started = Instant::now();
    let bytes = encode_artifact(&source, ArtifactOptions::default()).expect("Harbor encodes");
    let encode_ns = encode_started.elapsed().as_nanos();
    let mut measurements = Vec::new();
    for _ in 0..samples {
        let before_compile = AllocationSnapshot::now();
        let compile_started = Instant::now();
        let grammar = decode_artifact(&bytes, ArtifactLimits::default()).expect("Harbor decodes");
        let compile_ns = compile_started.elapsed().as_nanos();
        let after_compile = AllocationSnapshot::now();
        let (compile_calls, compile_bytes, compile_live) = after_compile.since(before_compile);
        let data = startup_data();
        let before_generation = AllocationSnapshot::now();
        let generation_started = Instant::now();
        let result = grammar
            .generate_weighted(&GenerationRequest {
                entry: Some("harbor.scene"),
                seed: 0,
                limits: workload_limits(),
                data: &data,
                trace_bindings: false,
                trace_selections: false,
                trace_provenance: false,
            })
            .expect("Harbor artifact generates");
        let generation_ns = generation_started.elapsed().as_nanos();
        let after_generation = AllocationSnapshot::now();
        let (generation_calls, generation_bytes, generation_live) =
            after_generation.since(before_generation);
        measurements.push(Measurement {
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
            expansions: u64::from(result.expansions()),
            sampler_words: u64::from(result.sampler_words()),
            output_bytes: result.text().len() as u64,
        });
    }
    let first = measurements[0];
    println!(
        "version,package,samples,artifact_bytes,encode_ns,load_ns_median,load_alloc_calls_median,load_alloc_bytes_median,load_live_bytes_median,first_generation_ns_median,first_generation_alloc_calls_median,first_generation_alloc_bytes_median,first_generation_live_bytes_median,rules,productions,expansions,sampler_words,output_bytes,artifact_hash"
    );
    println!(
        "{STARTUP_PROFILE_VERSION},{},{samples},{},{encode_ns},{},{},{},{},{},{},{},{},{},{},{},{},{},{:016x}",
        package.name,
        bytes.len(),
        median(&measurements, |sample| sample.compile_ns),
        median(&measurements, |sample| sample.compile_calls),
        median(&measurements, |sample| sample.compile_bytes),
        median(&measurements, |sample| sample.compile_live),
        median(&measurements, |sample| sample.generation_ns),
        median(&measurements, |sample| sample.generation_calls),
        median(&measurements, |sample| sample.generation_bytes),
        median(&measurements, |sample| sample.generation_live),
        first.rules,
        first.productions,
        first.expansions,
        first.sampler_words,
        first.output_bytes,
        source.artifact_hash(),
    );
}

fn startup_data() -> [DataBinding; 3] {
    [
        DataBinding::new("visitor".to_string(), Value::Text("Rin".to_string())),
        DataBinding::new("mood".to_string(), Value::Enum("tense".to_string())),
        DataBinding::new("urgency".to_string(), Value::Number(Rational::ONE)),
    ]
}

fn measure_startup() {
    let samples = 5;
    let mut measurements = Vec::new();
    let package = harbor_startup_package().expect("committed Harbor package loads");
    for _ in 0..samples {
        let before_compile = AllocationSnapshot::now();
        let compile_started = Instant::now();
        let grammar = compile_package_with_manifest(&package.input, &package.manifest)
            .expect("committed Harbor package compiles");
        let compile_ns = compile_started.elapsed().as_nanos();
        let after_compile = AllocationSnapshot::now();
        let (compile_calls, compile_bytes, compile_live) = after_compile.since(before_compile);
        let data = startup_data();
        let before_generation = AllocationSnapshot::now();
        let generation_started = Instant::now();
        let result = grammar
            .generate_weighted(&GenerationRequest {
                entry: Some("harbor.scene"),
                seed: 0,
                limits: workload_limits(),
                data: &data,
                trace_bindings: false,
                trace_selections: false,
                trace_provenance: false,
            })
            .expect("committed Harbor entry generates");
        let generation_ns = generation_started.elapsed().as_nanos();
        let after_generation = AllocationSnapshot::now();
        let (generation_calls, generation_bytes, generation_live) =
            after_generation.since(before_generation);
        measurements.push(Measurement {
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
            expansions: u64::from(result.expansions()),
            sampler_words: u64::from(result.sampler_words()),
            output_bytes: result.text().len() as u64,
        });
    }
    let first = measurements[0];
    println!(
        "version,package,samples,source_bytes,manifest_bytes,rules,productions,compile_ns_median,compile_alloc_calls_median,compile_alloc_bytes_median,compile_live_bytes_median,first_generation_ns_median,first_generation_alloc_calls_median,first_generation_alloc_bytes_median,first_generation_live_bytes_median,expansions,sampler_words,output_bytes,artifact_hash"
    );
    let grammar = compile_package_with_manifest(&package.input, &package.manifest)
        .expect("committed Harbor package compiles");
    println!(
        "{STARTUP_PROFILE_VERSION},{},{samples},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:016x}",
        package.name,
        package.source_bytes,
        package.manifest_bytes,
        first.rules,
        first.productions,
        median(&measurements, |sample| sample.compile_ns),
        median(&measurements, |sample| sample.compile_calls),
        median(&measurements, |sample| sample.compile_bytes),
        median(&measurements, |sample| sample.compile_live),
        median(&measurements, |sample| sample.generation_ns),
        median(&measurements, |sample| sample.generation_calls),
        median(&measurements, |sample| sample.generation_bytes),
        median(&measurements, |sample| sample.generation_live),
        first.expansions,
        first.sampler_words,
        first.output_bytes,
        grammar.artifact_hash(),
    );
}

fn measure(workload: &Workload) -> Measurement {
    let package = workload.package();
    let before_compile = AllocationSnapshot::now();
    let compile_started = Instant::now();
    let grammar = compile_package(&package).expect("committed workload compiles");
    let compile_ns = compile_started.elapsed().as_nanos();
    let after_compile = AllocationSnapshot::now();
    let (compile_calls, compile_bytes, compile_live) = after_compile.since(before_compile);

    measure_generation(
        workload,
        &grammar,
        compile_ns,
        compile_calls,
        compile_bytes,
        compile_live,
    )
}

fn measure_generation(
    workload: &Workload,
    grammar: &mecojoni_core::CompiledGrammar,
    compile_ns: u128,
    compile_calls: u64,
    compile_bytes: u64,
    compile_live: i128,
) -> Measurement {
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
