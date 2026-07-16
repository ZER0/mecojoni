use alloc::{
    collections::{BTreeMap, VecDeque},
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    CompiledGrammar, DataBinding, Diagnostic, DiagnosticCode, GenerationLimits, GenerationRequest,
    GenerationResult, LocationProfile, MecoError, MecoResult, Severity, SplitMix64,
};

/// Compatibility identifier for transactional repetition-resistant sampling.
pub const DIVERSE_SAMPLER_VERSION: &str = "diverse/1";
/// Locale-independent exact-output normalization contract.
pub const NORMALIZER_VERSION: &str = "ascii-fold-whitespace/1";
/// Locale-independent surface fragment contract.
pub const FRAGMENT_TOKENIZER_VERSION: &str = "scalar-word/1";
/// Binary compatibility version for session and repetition snapshots.
pub const SNAPSHOT_VERSION: u32 = 1;

const SESSION_SNAPSHOT_MAGIC: &[u8; 4] = b"MECS";
const REPETITION_SNAPSHOT_MAGIC: &[u8; 4] = b"MECR";
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// Explicit retention and sensitive-data policy for a restorable history snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotPolicy {
    pub max_logical_bytes: u64,
    pub pinned: bool,
    /// Revision-relative expiry; `None` keeps the snapshot until its owner drops it.
    pub expires_after_revisions: Option<u64>,
    /// Required because exact and fragment history may contain personal text.
    pub capture_sensitive: bool,
}

impl SnapshotPolicy {
    /// In-memory caller-owned snapshot with a fixed 64 MiB logical budget.
    pub const EPHEMERAL: Self = Self {
        max_logical_bytes: MAX_SNAPSHOT_BYTES as u64,
        pinned: false,
        expires_after_revisions: None,
        capture_sensitive: true,
    };
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self::EPHEMERAL
    }
}

/// Exact immutable cursor for one ordered sampler session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    version: u32,
    state: u64,
    words: u64,
}

impl SessionSnapshot {
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn state(&self) -> u64 {
        self.state
    }

    #[must_use]
    pub const fn words(&self) -> u64 {
        self.words
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(SESSION_SNAPSHOT_MAGIC);
        push_u32(&mut bytes, self.version);
        push_u64(&mut bytes, self.state);
        push_u64(&mut bytes, self.words);
        bytes
    }

    /// Decodes and validates a versioned session snapshot.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT` for malformed, trailing, or incompatible bytes.
    pub fn from_bytes(bytes: &[u8]) -> MecoResult<Self> {
        let mut decoder = SnapshotDecoder::new(bytes)?;
        decoder.magic(*SESSION_SNAPSHOT_MAGIC)?;
        let version = decoder.u32()?;
        if version != SNAPSHOT_VERSION {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                format!("unsupported session snapshot version {version}"),
            ));
        }
        let snapshot = Self {
            version,
            state: decoder.u64()?,
            words: decoder.u64()?,
        };
        decoder.finish()?;
        Ok(snapshot)
    }
}

/// Exact immutable repetition history plus its explicit retention metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepetitionSnapshot {
    version: u32,
    revision: u64,
    logical_bytes: u64,
    pinned: bool,
    expires_at_revision: Option<u64>,
    data: Rc<RepetitionData>,
}

impl RepetitionSnapshot {
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }

    #[must_use]
    pub const fn pinned(&self) -> bool {
        self.pinned
    }

    #[must_use]
    pub const fn expires_at_revision(&self) -> Option<u64> {
        self.expires_at_revision
    }

    #[must_use]
    pub fn is_expired(&self, observed_revision: u64) -> bool {
        self.expires_at_revision
            .is_some_and(|expiry| observed_revision > expiry)
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(REPETITION_SNAPSHOT_MAGIC);
        push_u32(&mut bytes, self.version);
        push_u64(&mut bytes, self.revision);
        push_u64(&mut bytes, self.logical_bytes);
        bytes.push(u8::from(self.pinned));
        push_optional_u64(&mut bytes, self.expires_at_revision);
        push_len(&mut bytes, self.data.structural.len());
        for (rule, productions) in &self.data.structural {
            push_string(&mut bytes, rule);
            push_len(&mut bytes, productions.len());
            for production in productions {
                push_string(&mut bytes, production);
            }
        }
        push_len(&mut bytes, self.data.exact.entries.len());
        for text in &self.data.exact.entries {
            push_string(&mut bytes, text);
        }
        push_len(&mut bytes, self.data.edges.entries.len());
        for phrase in &self.data.edges.entries {
            push_len(&mut bytes, phrase.len());
            for fragment in phrase {
                push_string(&mut bytes, fragment);
            }
        }
        bytes
    }

    /// Decodes and validates a bounded versioned repetition snapshot.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT` or `E_SNAPSHOT_LIMIT` without retaining partial state.
    pub fn from_bytes(bytes: &[u8]) -> MecoResult<Self> {
        let mut decoder = SnapshotDecoder::new(bytes)?;
        decoder.magic(*REPETITION_SNAPSHOT_MAGIC)?;
        let version = decoder.u32()?;
        if version != SNAPSHOT_VERSION {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                format!("unsupported repetition snapshot version {version}"),
            ));
        }
        let revision = decoder.u64()?;
        let declared_logical_bytes = decoder.u64()?;
        let pinned = decoder.boolean()?;
        let expires_at_revision = decoder.optional_u64()?;
        let structural_count = decoder.len()?;
        let mut structural = Vec::with_capacity(structural_count);
        for _ in 0..structural_count {
            let rule = decoder.string()?;
            let count = decoder.len()?;
            if count
                > usize::try_from(LocationProfile::V1.soft_cooldown_horizon).unwrap_or(usize::MAX)
            {
                return Err(snapshot_limit(
                    "structural history exceeds its profile window",
                ));
            }
            let mut productions = Vec::with_capacity(count);
            for _ in 0..count {
                productions.push(decoder.string()?);
            }
            structural.push((rule, productions));
        }
        let exact_count = decoder.len()?;
        if exact_count
            > usize::try_from(LocationProfile::V1.exact_history_window).unwrap_or(usize::MAX)
        {
            return Err(snapshot_limit("exact history exceeds its profile window"));
        }
        let mut exact = Vec::with_capacity(exact_count);
        for _ in 0..exact_count {
            exact.push(decoder.string()?);
        }
        if exact
            .iter()
            .map(|value| string_bytes(value))
            .fold(0_u64, u64::saturating_add)
            > LocationProfile::V1.exact_history_logical_bytes
        {
            return Err(snapshot_limit(
                "exact history exceeds its profile logical-byte budget",
            ));
        }
        let edge_count = decoder.len()?;
        if edge_count
            > usize::try_from(LocationProfile::V1.edge_history_window).unwrap_or(usize::MAX)
        {
            return Err(snapshot_limit("edge history exceeds its profile window"));
        }
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            let fragment_count = decoder.len()?;
            let mut phrase = Vec::with_capacity(fragment_count);
            for _ in 0..fragment_count {
                phrase.push(decoder.string()?);
            }
            edges.push(phrase);
        }
        if edges
            .iter()
            .flat_map(|phrase| phrase.iter())
            .map(|value| string_bytes(value))
            .fold(0_u64, u64::saturating_add)
            > LocationProfile::V1.edge_history_logical_bytes
        {
            return Err(snapshot_limit(
                "edge history exceeds its profile logical-byte budget",
            ));
        }
        decoder.finish()?;
        let logical_bytes = repetition_logical_bytes(&structural, &exact, &edges);
        if logical_bytes != declared_logical_bytes {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "repetition snapshot logical-byte declaration does not match its payload",
            ));
        }
        Ok(Self {
            version,
            revision,
            logical_bytes,
            pinned,
            expires_at_revision,
            data: Rc::new(RepetitionData::from_snapshot_parts(
                revision, structural, exact, edges,
            )),
        })
    }

    #[must_use]
    pub fn content_hash(&self) -> u64 {
        hash_bytes(&self.to_bytes())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CountMap {
    slots: Vec<CountSlot>,
    len: usize,
    tombstones: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CountSlot {
    key: Option<String>,
    count: u32,
    tombstone: bool,
}

impl CountMap {
    fn get(&self, key: &str) -> u32 {
        self.find(key).map_or(0, |index| self.slots[index].count)
    }

    fn increment(&mut self, key: String) {
        self.reserve_entry();
        if let Some(index) = self.find(&key) {
            self.slots[index].count = self.slots[index].count.saturating_add(1);
            return;
        }
        self.insert_new(key, 1);
    }

    fn decrement(&mut self, key: &str) {
        let index = self.find(key).expect("history key is counted");
        if self.slots[index].count > 1 {
            self.slots[index].count -= 1;
            return;
        }
        self.slots[index].key = None;
        self.slots[index].count = 0;
        self.slots[index].tombstone = true;
        self.len -= 1;
        self.tombstones += 1;
    }

    fn find(&self, key: &str) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut index = hash_index(key) & mask;
        for _ in 0..self.slots.len() {
            let slot = &self.slots[index];
            if slot.key.as_deref() == Some(key) {
                return Some(index);
            }
            if slot.key.is_none() && !slot.tombstone {
                return None;
            }
            index = (index + 1) & mask;
        }
        None
    }

    fn reserve_entry(&mut self) {
        if self.slots.is_empty() {
            self.rehash(16);
        } else if (self.len + self.tombstones + 1) * 10 >= self.slots.len() * 7 {
            self.rehash(self.slots.len() * 2);
        }
    }

    fn rehash(&mut self, capacity: usize) {
        let old = core::mem::replace(
            &mut self.slots,
            (0..capacity).map(|_| CountSlot::default()).collect(),
        );
        self.len = 0;
        self.tombstones = 0;
        for slot in old {
            if let Some(key) = slot.key {
                self.insert_new(key, slot.count);
            }
        }
    }

    fn insert_new(&mut self, key: String, count: u32) {
        let mask = self.slots.len() - 1;
        let mut index = hash_index(&key) & mask;
        let mut tombstone = None;
        loop {
            let slot = &self.slots[index];
            if slot.key.is_none() {
                if slot.tombstone {
                    tombstone.get_or_insert(index);
                } else {
                    let target = tombstone.unwrap_or(index);
                    if self.slots[target].tombstone {
                        self.tombstones -= 1;
                    }
                    self.slots[target] = CountSlot {
                        key: Some(key),
                        count,
                        tombstone: false,
                    };
                    self.len += 1;
                    return;
                }
            }
            index = (index + 1) & mask;
        }
    }
}

fn hash_key(key: &str) -> u64 {
    key.as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn hash_index(key: &str) -> usize {
    let hash = hash_key(key);
    usize::try_from(hash).unwrap_or_else(|_| {
        usize::try_from((hash ^ (hash >> 32)) & u64::from(u32::MAX))
            .expect("folded hash fits a 32-bit usize")
    })
}

/// One stateful request. Randomness comes exclusively from its sampler session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct DiverseGenerationRequest<'a> {
    pub entry: Option<&'a str>,
    pub limits: GenerationLimits,
    pub data: &'a [DataBinding],
    pub trace_bindings: bool,
    pub trace_selections: bool,
    pub trace_provenance: bool,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CountedHistory {
    capacity: usize,
    byte_capacity: u64,
    logical_bytes: u64,
    entries: VecDeque<String>,
    counts: CountMap,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FragmentHistory {
    capacity: usize,
    byte_capacity: u64,
    logical_bytes: u64,
    entries: VecDeque<Vec<String>>,
    counts: CountMap,
}

impl FragmentHistory {
    fn with_limits(capacity: u32, byte_capacity: u64) -> Self {
        Self {
            capacity: usize::try_from(capacity).unwrap_or(usize::MAX),
            byte_capacity,
            logical_bytes: 0,
            entries: VecDeque::new(),
            counts: CountMap::default(),
        }
    }

    fn count(&self, value: &str) -> u32 {
        self.counts.get(value)
    }

    fn push(&mut self, fragments: Vec<String>) {
        if self.capacity == 0 {
            return;
        }
        for fragment in &fragments {
            self.counts.increment(fragment.clone());
            self.logical_bytes = self.logical_bytes.saturating_add(string_bytes(fragment));
        }
        self.entries.push_back(fragments);
        while self.entries.len() > self.capacity || self.logical_bytes > self.byte_capacity {
            for fragment in self
                .entries
                .pop_front()
                .expect("overfull fragment history has a head")
            {
                self.logical_bytes = self.logical_bytes.saturating_sub(string_bytes(&fragment));
                self.counts.decrement(&fragment);
            }
        }
    }
}

impl CountedHistory {
    fn with_limits(capacity: u32, byte_capacity: u64) -> Self {
        Self {
            capacity: usize::try_from(capacity).unwrap_or(usize::MAX),
            byte_capacity,
            logical_bytes: 0,
            entries: VecDeque::new(),
            counts: CountMap::default(),
        }
    }

    fn count(&self, value: &str) -> u32 {
        self.counts.get(value)
    }

    fn push(&mut self, value: String) {
        if self.capacity == 0 {
            return;
        }
        self.counts.increment(value.clone());
        self.logical_bytes = self.logical_bytes.saturating_add(string_bytes(&value));
        self.entries.push_back(value);
        while self.entries.len() > self.capacity || self.logical_bytes > self.byte_capacity {
            let oldest = self
                .entries
                .pop_front()
                .expect("overfull history has a head");
            self.logical_bytes = self.logical_bytes.saturating_sub(string_bytes(&oldest));
            self.counts.decrement(&oldest);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepetitionData {
    revision: u64,
    structural: BTreeMap<String, VecDeque<String>>,
    exact: CountedHistory,
    edges: FragmentHistory,
}

impl RepetitionData {
    fn new_location() -> Self {
        let profile = LocationProfile::V1;
        Self {
            revision: 0,
            structural: BTreeMap::new(),
            exact: CountedHistory::with_limits(
                profile.exact_history_window,
                profile.exact_history_logical_bytes,
            ),
            edges: FragmentHistory::with_limits(
                profile.edge_history_window,
                profile.edge_history_logical_bytes,
            ),
        }
    }

    fn from_snapshot_parts(
        revision: u64,
        structural: Vec<(String, Vec<String>)>,
        exact: Vec<String>,
        edges: Vec<Vec<String>>,
    ) -> Self {
        let mut data = Self::new_location();
        data.revision = revision;
        for (rule, productions) in structural {
            data.structural
                .insert(rule, productions.into_iter().collect());
        }
        for text in exact {
            data.exact.push(text);
        }
        for phrase in edges {
            data.edges.push(phrase);
        }
        data
    }
}

/// Mutable repetition domain shared explicitly by one or more sampler sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepetitionStore {
    busy: bool,
    data: Rc<RepetitionData>,
}

impl Default for RepetitionStore {
    fn default() -> Self {
        Self::new_location()
    }
}

impl RepetitionStore {
    #[must_use]
    pub fn new_location() -> Self {
        Self {
            busy: false,
            data: Rc::new(RepetitionData::new_location()),
        }
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.data.revision
    }

    /// Estimated canonical UTF-8 payload retained by live novelty histories.
    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        repetition_data_logical_bytes(&self.data)
    }

    fn state_hash(&self) -> u64 {
        let mut hash = ReplayHasher::new("repetition-state/1");
        hash.u64(self.data.revision);
        for (rule, productions) in &self.data.structural {
            hash.string(rule);
            hash.u64(u64::try_from(productions.len()).unwrap_or(u64::MAX));
            for production in productions {
                hash.string(production);
            }
        }
        hash.u64(u64::try_from(self.data.exact.entries.len()).unwrap_or(u64::MAX));
        for text in &self.data.exact.entries {
            hash.string(text);
        }
        hash.u64(u64::try_from(self.data.edges.entries.len()).unwrap_or(u64::MAX));
        for phrase in &self.data.edges.entries {
            hash.u64(u64::try_from(phrase.len()).unwrap_or(u64::MAX));
            for fragment in phrase {
                hash.string(fragment);
            }
        }
        hash.finish()
    }

    /// Captures an immutable restorable snapshot under an explicit budget and
    /// sensitive-history policy.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_LIMIT` when consent is absent or the logical budget
    /// cannot hold the complete transactional state.
    pub fn snapshot_with_policy(&self, policy: SnapshotPolicy) -> MecoResult<RepetitionSnapshot> {
        if !policy.capture_sensitive {
            return Err(snapshot_limit(
                "restorable snapshots require explicit sensitive-history consent",
            ));
        }
        let logical_bytes = repetition_data_logical_bytes(&self.data);
        if logical_bytes > policy.max_logical_bytes {
            return Err(snapshot_limit(format!(
                "repetition snapshot requires {logical_bytes} logical bytes but the policy allows {}",
                policy.max_logical_bytes
            )));
        }
        Ok(RepetitionSnapshot {
            version: SNAPSHOT_VERSION,
            revision: self.data.revision,
            logical_bytes,
            pinned: policy.pinned,
            expires_at_revision: policy
                .expires_after_revisions
                .map(|delta| self.data.revision.saturating_add(delta)),
            data: Rc::clone(&self.data),
        })
    }

    /// Captures the default caller-owned in-memory snapshot.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_LIMIT` if the 64 MiB profile budget is exceeded.
    pub fn snapshot(&self) -> MecoResult<RepetitionSnapshot> {
        self.snapshot_with_policy(SnapshotPolicy::EPHEMERAL)
    }

    /// Restores a snapshot while checking its revision-relative expiry.
    ///
    /// # Errors
    ///
    /// Returns a stable snapshot diagnostic for incompatible or expired state.
    pub fn restore_at(snapshot: &RepetitionSnapshot, observed_revision: u64) -> MecoResult<Self> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "incompatible repetition snapshot version",
            ));
        }
        if snapshot.is_expired(observed_revision) {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT_EXPIRED,
                "repetition snapshot expired under its revision policy",
            ));
        }
        Ok(Self {
            busy: false,
            data: Rc::clone(&snapshot.data),
        })
    }

    /// Restores an unexpired snapshot at its own captured revision.
    ///
    /// # Errors
    ///
    /// Returns a stable snapshot diagnostic for incompatible state.
    pub fn restore(snapshot: &RepetitionSnapshot) -> MecoResult<Self> {
        Self::restore_at(snapshot, snapshot.revision)
    }

    #[must_use]
    pub fn exact_len(&self) -> usize {
        self.data.exact.entries.len()
    }

    #[must_use]
    pub fn edge_len(&self) -> usize {
        self.data.edges.entries.len()
    }

    pub(crate) fn structural_history(&self, rule: &str) -> Option<&VecDeque<String>> {
        self.data.structural.get(rule)
    }

    pub(crate) fn exact_count(&self, text: &str) -> u32 {
        self.data.exact.count(&normalize_text(text))
    }

    pub(crate) fn edge_score(&self, text: &str) -> u64 {
        edge_fragments(text)
            .iter()
            .map(|fragment| u64::from(self.data.edges.count(fragment)))
            .sum()
    }

    fn commit(&mut self, selections: &[(String, String)], text: &str) {
        let data = Rc::make_mut(&mut self.data);
        let horizon =
            usize::try_from(LocationProfile::V1.soft_cooldown_horizon).unwrap_or(usize::MAX);
        for (rule, production) in selections {
            let history = data.structural.entry(rule.clone()).or_default();
            history.push_back(production.clone());
            while history.len() > horizon {
                history.pop_front();
            }
        }
        data.exact.push(normalize_text(text));
        data.edges.push(edge_fragments(text));
        data.revision = data.revision.wrapping_add(1);
    }
}

/// One ordered deterministic random stream. Failed calls leave it unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SamplerSession {
    random: SplitMix64,
    busy: bool,
}

impl SamplerSession {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            random: SplitMix64::new(seed),
            busy: false,
        }
    }

    #[must_use]
    pub const fn random_state(&self) -> u64 {
        self.random.state()
    }

    #[must_use]
    pub const fn random_words(&self) -> u64 {
        self.random.words()
    }

    #[must_use]
    pub const fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            version: SNAPSHOT_VERSION,
            state: self.random.state(),
            words: self.random.words(),
        }
    }

    /// Restores an exact ordered random cursor.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT` for an incompatible snapshot version.
    pub fn restore(snapshot: SessionSnapshot) -> MecoResult<Self> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "incompatible session snapshot version",
            ));
        }
        Ok(Self {
            random: SplitMix64::from_state(snapshot.state, snapshot.words),
            busy: false,
        })
    }

    /// Runs all `location/1` attempts against one snapshot and commits only the
    /// selected candidate.
    ///
    /// # Errors
    ///
    /// Returns stable request/generation diagnostics. Busy or failed calls do
    /// not advance the session or repetition revision.
    pub fn generate(
        &mut self,
        grammar: &CompiledGrammar,
        store: &mut RepetitionStore,
        request: &DiverseGenerationRequest<'_>,
    ) -> MecoResult<DiverseResult> {
        if request.cancelled {
            return Err(state_error(
                DiagnosticCode::CANCELLED,
                "diverse generation was cancelled before candidate reservation",
            ));
        }
        if self.busy || store.busy {
            return Err(state_error(
                DiagnosticCode::STATE_BUSY,
                "sampler session or repetition store already has an active transaction",
            ));
        }
        self.busy = true;
        store.busy = true;
        let result = self.generate_transaction(grammar, store, request);
        self.busy = false;
        store.busy = false;
        result
    }

    fn generate_transaction(
        &mut self,
        grammar: &CompiledGrammar,
        store: &mut RepetitionStore,
        request: &DiverseGenerationRequest<'_>,
    ) -> MecoResult<DiverseResult> {
        let attempts = LocationProfile::V1.candidate_attempts;
        let pre_session = self.snapshot();
        let pre_repetition_revision = store.revision();
        let pre_repetition_hash = store.state_hash();
        let mut reserved = self.random;
        let seeds = (0..attempts)
            .map(|_| reserved.next_u64())
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        let mut first_error = None;
        for (attempt, seed) in seeds.into_iter().enumerate() {
            let mut state = DiverseCandidateState::new(store);
            let candidate_request = GenerationRequest {
                entry: request.entry,
                seed,
                limits: request.limits,
                data: request.data,
                trace_bindings: request.trace_bindings,
                trace_selections: request.trace_selections,
                trace_provenance: request.trace_provenance,
            };
            match grammar.generate_diverse_candidate(&candidate_request, &mut state) {
                Ok(generation) => {
                    let ranking = (
                        store.exact_count(generation.text()),
                        store.edge_score(generation.text()),
                        u32::try_from(attempt).unwrap_or(u32::MAX),
                    );
                    candidates.push((ranking, generation, state.selections));
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        let Some((ranking, generation, selections)) = candidates
            .into_iter()
            .min_by_key(|(ranking, _, _)| *ranking)
        else {
            return Err(first_error.unwrap_or_else(|| {
                state_error(
                    DiagnosticCode::NO_ELIGIBLE_PRODUCTION,
                    "all diverse candidates were disqualified",
                )
            }));
        };
        let derivation_hash = selections_hash(&selections);
        store.commit(&selections, generation.text());
        self.random = reserved;
        let receipt = ReplayReceipt {
            version: SNAPSHOT_VERSION,
            grammar_hash: grammar.artifact_hash(),
            sampler_version: DIVERSE_SAMPLER_VERSION,
            normalizer_version: NORMALIZER_VERSION,
            tokenizer_version: FRAGMENT_TOKENIZER_VERSION,
            pre_session_hash: session_state_hash(pre_session),
            pre_session_words: pre_session.words,
            pre_repetition_hash,
            pre_repetition_revision,
            reserved_words: u64::from(attempts),
            request_digest: request_digest(request),
            effective_entry: generation.entry().to_string(),
            winner_attempt: ranking.2,
            derivation_hash,
            final_text_hash: hash_bytes(generation.text().as_bytes()),
            post_session_hash: session_state_hash(self.snapshot()),
            post_repetition_hash: store.state_hash(),
            post_repetition_revision: store.revision(),
        };
        Ok(DiverseResult {
            generation,
            attempts,
            winner_attempt: ranking.2,
            exact_repetitions: ranking.0,
            edge_repetitions: ranking.1,
            committed_revision: store.revision(),
            receipt,
        })
    }
}

/// Successful transactional diverse generation and deterministic score facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiverseResult {
    generation: GenerationResult,
    attempts: u32,
    winner_attempt: u32,
    exact_repetitions: u32,
    edge_repetitions: u64,
    committed_revision: u64,
    receipt: ReplayReceipt,
}

impl DiverseResult {
    #[must_use]
    pub const fn generation(&self) -> &GenerationResult {
        &self.generation
    }

    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    #[must_use]
    pub const fn winner_attempt(&self) -> u32 {
        self.winner_attempt
    }

    #[must_use]
    pub const fn exact_repetitions(&self) -> u32 {
        self.exact_repetitions
    }

    #[must_use]
    pub const fn edge_repetitions(&self) -> u64 {
        self.edge_repetitions
    }

    #[must_use]
    pub const fn committed_revision(&self) -> u64 {
        self.committed_revision
    }

    #[must_use]
    pub const fn receipt(&self) -> &ReplayReceipt {
        &self.receipt
    }
}

/// Verification record for one committed deterministic generation transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayReceipt {
    version: u32,
    grammar_hash: u64,
    sampler_version: &'static str,
    normalizer_version: &'static str,
    tokenizer_version: &'static str,
    pre_session_hash: u64,
    pre_session_words: u64,
    pre_repetition_hash: u64,
    pre_repetition_revision: u64,
    reserved_words: u64,
    request_digest: u64,
    effective_entry: String,
    winner_attempt: u32,
    derivation_hash: u64,
    final_text_hash: u64,
    post_session_hash: u64,
    post_repetition_hash: u64,
    post_repetition_revision: u64,
}

impl ReplayReceipt {
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn grammar_hash(&self) -> u64 {
        self.grammar_hash
    }

    #[must_use]
    pub const fn sampler_version(&self) -> &str {
        self.sampler_version
    }

    #[must_use]
    pub const fn normalizer_version(&self) -> &str {
        self.normalizer_version
    }

    #[must_use]
    pub const fn tokenizer_version(&self) -> &str {
        self.tokenizer_version
    }

    #[must_use]
    pub const fn pre_session_hash(&self) -> u64 {
        self.pre_session_hash
    }

    #[must_use]
    pub const fn pre_session_words(&self) -> u64 {
        self.pre_session_words
    }

    #[must_use]
    pub const fn pre_repetition_hash(&self) -> u64 {
        self.pre_repetition_hash
    }

    #[must_use]
    pub const fn pre_repetition_revision(&self) -> u64 {
        self.pre_repetition_revision
    }

    #[must_use]
    pub const fn reserved_words(&self) -> u64 {
        self.reserved_words
    }

    #[must_use]
    pub const fn request_digest(&self) -> u64 {
        self.request_digest
    }

    #[must_use]
    pub fn effective_entry(&self) -> &str {
        &self.effective_entry
    }

    #[must_use]
    pub const fn winner_attempt(&self) -> u32 {
        self.winner_attempt
    }

    #[must_use]
    pub const fn derivation_hash(&self) -> u64 {
        self.derivation_hash
    }

    #[must_use]
    pub const fn final_text_hash(&self) -> u64 {
        self.final_text_hash
    }

    #[must_use]
    pub const fn post_session_hash(&self) -> u64 {
        self.post_session_hash
    }

    #[must_use]
    pub const fn post_repetition_hash(&self) -> u64 {
        self.post_repetition_hash
    }

    #[must_use]
    pub const fn post_repetition_revision(&self) -> u64 {
        self.post_repetition_revision
    }
}

pub(crate) struct DiverseCandidateState<'a> {
    store: &'a RepetitionStore,
    pub(crate) selections: Vec<(String, String)>,
}

impl<'a> DiverseCandidateState<'a> {
    fn new(store: &'a RepetitionStore) -> Self {
        Self {
            store,
            selections: Vec::new(),
        }
    }

    pub(crate) fn recent(&self, rule: &str) -> Vec<&str> {
        self.store
            .structural_history(rule)
            .into_iter()
            .flat_map(|history| history.iter().map(String::as_str))
            .chain(
                self.selections
                    .iter()
                    .filter(|(candidate, _)| candidate == rule)
                    .map(|(_, production)| production.as_str()),
            )
            .collect()
    }

    pub(crate) fn selection_age(&self, rule: &str, production: &str) -> Option<u32> {
        self.recent(rule)
            .iter()
            .rev()
            .position(|candidate| *candidate == production)
            .map(|age| u32::try_from(age + 1).unwrap_or(u32::MAX))
    }

    pub(crate) fn record(&mut self, rule: &str, production: &str) {
        self.selections
            .push((rule.to_string(), production.to_string()));
    }
}

fn normalize_text(text: &str) -> String {
    let mut normalized = String::new();
    let mut separator = false;
    for character in text.chars() {
        if character.is_whitespace() {
            separator = !normalized.is_empty();
        } else {
            if separator {
                normalized.push(' ');
                separator = false;
            }
            normalized.push(if character.is_ascii_uppercase() {
                character.to_ascii_lowercase()
            } else {
                character
            });
        }
    }
    normalized
}

fn edge_fragments(text: &str) -> Vec<String> {
    let words = word_tokens(text);
    let profile = LocationProfile::V1;
    let minimum = usize::try_from(profile.minimum_edge_words).unwrap_or(usize::MAX);
    let maximum = usize::try_from(profile.maximum_edge_words).unwrap_or(usize::MAX);
    let mut fragments = Vec::new();
    for length in minimum..=maximum.min(words.len()) {
        fragments.push(words[..length].join(" "));
        if length < words.len() {
            fragments.push(words[words.len() - length..].join(" "));
        }
    }
    let internal = usize::try_from(profile.internal_boundary_words).unwrap_or(usize::MAX);
    let sentences = text
        .split(['.', '!', '?'])
        .map(word_tokens)
        .filter(|sentence| !sentence.is_empty())
        .collect::<Vec<_>>();
    for boundary in sentences.windows(2) {
        let left = &boundary[0];
        let right = &boundary[1];
        if left.len() >= internal {
            fragments.push(left[left.len() - internal..].join(" "));
        }
        if right.len() >= internal {
            fragments.push(right[..internal].join(" "));
        }
    }
    fragments
}

fn word_tokens(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for character in text.chars() {
        if is_word_scalar(character) {
            word.push(if character.is_ascii_uppercase() {
                character.to_ascii_lowercase()
            } else {
                character
            });
        } else if !word.is_empty() {
            words.push(core::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn is_word_scalar(character: char) -> bool {
    if character.is_ascii() {
        return character.is_ascii_alphanumeric() || character == '_';
    }
    !matches!(
        character as u32,
        0x0085
            | 0x00a0
            | 0x1680
            | 0x3000
            | 0x2000..=0x206f
            | 0x2e00..=0x2e7f
            | 0x3001..=0x303f
            | 0xfe10..=0xfe1f
            | 0xfe30..=0xfe4f
            | 0xff01..=0xff0f
            | 0xff1a..=0xff20
            | 0xff3b..=0xff40
            | 0xff5b..=0xff65
    )
}

fn repetition_logical_bytes(
    structural: &[(String, Vec<String>)],
    exact: &[String],
    edges: &[Vec<String>],
) -> u64 {
    structural
        .iter()
        .map(|(rule, productions)| {
            string_bytes(rule)
                .saturating_add(productions.iter().map(|value| string_bytes(value)).sum())
        })
        .chain(exact.iter().map(|value| string_bytes(value)))
        .chain(
            edges
                .iter()
                .flat_map(|phrase| phrase.iter().map(|value| string_bytes(value))),
        )
        .fold(0_u64, u64::saturating_add)
}

fn repetition_data_logical_bytes(data: &RepetitionData) -> u64 {
    data.structural
        .iter()
        .map(|(rule, productions)| {
            string_bytes(rule).saturating_add(
                productions
                    .iter()
                    .map(|value| string_bytes(value))
                    .fold(0_u64, u64::saturating_add),
            )
        })
        .chain(data.exact.entries.iter().map(|value| string_bytes(value)))
        .chain(
            data.edges
                .entries
                .iter()
                .flat_map(|phrase| phrase.iter().map(|value| string_bytes(value))),
        )
        .fold(0_u64, u64::saturating_add)
}

fn string_bytes(value: &str) -> u64 {
    u64::try_from(value.len()).unwrap_or(u64::MAX)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

struct ReplayHasher(u64);

impl ReplayHasher {
    fn new(domain: &str) -> Self {
        let mut value = Self(0xcbf2_9ce4_8422_2325);
        value.string(domain);
        value
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0 = bytes.iter().fold(self.0, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u64(u64::try_from(value.len()).unwrap_or(u64::MAX));
        self.bytes(value.as_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

fn session_state_hash(snapshot: SessionSnapshot) -> u64 {
    let mut hash = ReplayHasher::new("session-state/1");
    hash.u64(u64::from(snapshot.version));
    hash.u64(snapshot.state);
    hash.u64(snapshot.words);
    hash.finish()
}

fn selections_hash(selections: &[(String, String)]) -> u64 {
    let mut hash = ReplayHasher::new("derivation/1");
    for (rule, production) in selections {
        hash.string(rule);
        hash.string(production);
    }
    hash.finish()
}

fn request_digest(request: &DiverseGenerationRequest<'_>) -> u64 {
    let mut hash = ReplayHasher::new("diverse-request/1");
    hash.string(request.entry.unwrap_or(""));
    hash.u64(u64::from(request.limits.max_depth));
    hash.u64(u64::from(request.limits.max_expansions));
    hash.u64(u64::from(request.limits.max_output_scalars));
    hash.u64(u64::from(request.limits.max_output_bytes));
    hash.u64(u64::from(request.limits.max_sampler_words));
    let mut data = request.data.iter().collect::<Vec<_>>();
    data.sort_by(|left, right| left.name.cmp(&right.name));
    for binding in data {
        hash.string(&binding.name);
        match &binding.value {
            crate::Value::Text(value) => {
                hash.u64(0);
                hash.string(value);
            }
            crate::Value::Number(value) => {
                hash.u64(1);
                hash.bytes(&value.numerator().to_le_bytes());
                hash.u64(value.denominator());
            }
            crate::Value::Boolean(value) => {
                hash.u64(2);
                hash.u64(u64::from(*value));
            }
            crate::Value::Enum(value) => {
                hash.u64(3);
                hash.string(value);
            }
        }
    }
    hash.u64(u64::from(request.trace_bindings));
    hash.u64(u64::from(request.trace_selections));
    hash.u64(u64::from(request.trace_provenance));
    hash.finish()
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_len(output: &mut Vec<u8>, value: usize) {
    push_u32(output, u32::try_from(value).unwrap_or(u32::MAX));
}

fn push_string(output: &mut Vec<u8>, value: &str) {
    push_len(output, value.len());
    output.extend_from_slice(value.as_bytes());
}

fn push_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    output.push(u8::from(value.is_some()));
    if let Some(value) = value {
        push_u64(output, value);
    }
}

struct SnapshotDecoder<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> SnapshotDecoder<'a> {
    fn new(bytes: &'a [u8]) -> MecoResult<Self> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(snapshot_limit("snapshot exceeds the 64 MiB decode budget"));
        }
        Ok(Self { bytes, cursor: 0 })
    }

    fn take(&mut self, length: usize) -> MecoResult<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or_else(|| snapshot_error(DiagnosticCode::SNAPSHOT, "snapshot range overflowed"))?;
        let value = self.bytes.get(self.cursor..end).ok_or_else(|| {
            snapshot_error(DiagnosticCode::SNAPSHOT, "snapshot ended unexpectedly")
        })?;
        self.cursor = end;
        Ok(value)
    }

    fn magic(&mut self, expected: [u8; 4]) -> MecoResult<()> {
        if self.take(4)? == expected {
            Ok(())
        } else {
            Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "snapshot magic does not match its requested state kind",
            ))
        }
    }

    fn u32(&mut self) -> MecoResult<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("decoder returned four bytes"),
        ))
    }

    fn u64(&mut self) -> MecoResult<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("decoder returned eight bytes"),
        ))
    }

    fn boolean(&mut self) -> MecoResult<bool> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "snapshot boolean is not 0 or 1",
            )),
        }
    }

    fn optional_u64(&mut self) -> MecoResult<Option<u64>> {
        if self.boolean()? {
            Ok(Some(self.u64()?))
        } else {
            Ok(None)
        }
    }

    fn len(&mut self) -> MecoResult<usize> {
        usize::try_from(self.u32()?)
            .map_err(|_| snapshot_limit("snapshot collection length exceeds this target"))
    }

    fn string(&mut self) -> MecoResult<String> {
        let length = self.len()?;
        let bytes = self.take(length)?;
        let value = core::str::from_utf8(bytes).map_err(|_| {
            snapshot_error(DiagnosticCode::SNAPSHOT, "snapshot string is not UTF-8")
        })?;
        Ok(value.to_string())
    }

    fn finish(self) -> MecoResult<()> {
        if self.cursor == self.bytes.len() {
            Ok(())
        } else {
            Err(snapshot_error(
                DiagnosticCode::SNAPSHOT,
                "snapshot contains trailing bytes",
            ))
        }
    }
}

fn snapshot_limit(message: impl Into<String>) -> MecoError {
    snapshot_error(DiagnosticCode::SNAPSHOT_LIMIT, message)
}

fn snapshot_error(code: DiagnosticCode, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

fn state_error(code: DiagnosticCode, message: &str) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{
        CountedHistory, DiverseGenerationRequest, FragmentHistory, RepetitionSnapshot,
        RepetitionStore, SamplerSession, SessionSnapshot, SnapshotPolicy, edge_fragments,
        normalize_text,
    };
    use crate::{
        DiagnosticCode, LocationProfile, PackageInput, PackageSource, SourceFile, SourceId,
        compile_package,
    };

    #[test]
    fn counted_history_evicts_without_shifting_and_retains_duplicate_counts() {
        let mut history = CountedHistory::with_limits(2, u64::MAX);
        history.push("a".to_string());
        history.push("a".to_string());
        history.push("b".to_string());
        assert_eq!(history.count("a"), 1);
        assert_eq!(history.count("b"), 1);

        let mut byte_bounded = CountedHistory::with_limits(10, 3);
        byte_bounded.push("aa".to_string());
        byte_bounded.push("bb".to_string());
        assert_eq!(byte_bounded.entries.len(), 1);
        assert_eq!(byte_bounded.count("aa"), 0);
        assert_eq!(byte_bounded.count("bb"), 1);
    }

    #[test]
    fn location_normalization_and_edges_are_deterministic() {
        assert_eq!(normalize_text("  HELLO   World  "), "hello world");
        assert_eq!(
            edge_fragments("one two three four"),
            ["one two three", "two three four", "one two three four"]
        );
        assert!(edge_fragments("zero one two. three four five.").contains(&"one two".to_string()));
    }

    #[test]
    fn location_exact_window_evicts_at_its_published_boundary() {
        let mut history =
            CountedHistory::with_limits(LocationProfile::V1.exact_history_window, u64::MAX);
        for index in 0..=LocationProfile::V1.exact_history_window {
            history.push(index.to_string());
        }
        assert_eq!(history.entries.len(), 50_000);
        assert_eq!(history.count("0"), 0);
        assert_eq!(history.count("1"), 1);
    }

    #[test]
    fn edge_window_counts_phrases_and_evicts_all_of_their_fragments() {
        let mut history = FragmentHistory::with_limits(2, u64::MAX);
        history.push(alloc::vec!["a".to_string(), "shared".to_string()]);
        history.push(alloc::vec!["b".to_string(), "shared".to_string()]);
        history.push(alloc::vec!["c".to_string()]);
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.count("a"), 0);
        assert_eq!(history.count("shared"), 1);

        let mut location =
            FragmentHistory::with_limits(LocationProfile::V1.edge_history_window, u64::MAX);
        for index in 0..=LocationProfile::V1.edge_history_window {
            location.push(alloc::vec![index.to_string()]);
        }
        assert_eq!(location.entries.len(), 300);
        assert_eq!(location.count("0"), 0);
    }

    #[test]
    fn overlapping_session_or_store_transactions_are_rejected() {
        let source = SourceFile::new(
            SourceId::new(0),
            "busy.meco.md",
            "---\nmeco: 2\nmodule: busy\nentry: line\nexports: [line]\n---\n# line\n- ok\n",
        );
        let grammar = compile_package(&PackageInput {
            root_id: "root".to_string(),
            modules: alloc::vec![PackageSource {
                canonical_id: "root".to_string(),
                source,
                resolved_imports: alloc::vec![],
            }],
        })
        .expect("busy fixture compiles");
        let mut session = SamplerSession::new(0);
        let mut store = RepetitionStore::new_location();
        session.busy = true;
        let error = session
            .generate(&grammar, &mut store, &DiverseGenerationRequest::default())
            .expect_err("overlapping session fails");
        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::STATE_BUSY);
        session.busy = false;
        store.busy = true;
        let error = session
            .generate(&grammar, &mut store, &DiverseGenerationRequest::default())
            .expect_err("overlapping store fails");
        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::STATE_BUSY);
    }

    #[test]
    fn nonempty_snapshots_round_trip_and_reproduce_the_next_output() {
        let source = SourceFile::new(
            SourceId::new(0),
            "snapshot.meco.md",
            concat!(
                "---\nmeco: 2\nmodule: snapshot\nentry: line\nexports: [line]\n---\n",
                "# line\n- Alpha signal.\n- Beta signal.\n- Gamma signal.\n",
            ),
        );
        let grammar = compile_package(&PackageInput {
            root_id: "root".to_string(),
            modules: alloc::vec![PackageSource {
                canonical_id: "root".to_string(),
                source,
                resolved_imports: alloc::vec![],
            }],
        })
        .expect("snapshot fixture compiles");
        let mut session = SamplerSession::new(73);
        let mut store = RepetitionStore::new_location();
        session
            .generate(&grammar, &mut store, &DiverseGenerationRequest::default())
            .expect("initial call populates history");
        let session_snapshot = SessionSnapshot::from_bytes(&session.snapshot().to_bytes())
            .expect("session snapshot round-trips");
        let shared_snapshot = store.snapshot().expect("snapshot");
        assert!(alloc::rc::Rc::ptr_eq(&shared_snapshot.data, &store.data));
        let repetition_snapshot = RepetitionSnapshot::from_bytes(&shared_snapshot.to_bytes())
            .expect("repetition snapshot round-trips");

        let expected = session
            .generate(&grammar, &mut store, &DiverseGenerationRequest::default())
            .expect("original continuation");
        let mut restored_session =
            SamplerSession::restore(session_snapshot).expect("session restores");
        let mut restored_store =
            RepetitionStore::restore(&repetition_snapshot).expect("history restores");
        let replayed = restored_session
            .generate(
                &grammar,
                &mut restored_store,
                &DiverseGenerationRequest::default(),
            )
            .expect("restored continuation");

        assert_eq!(replayed, expected);
        assert_eq!(restored_store, store);
        assert_eq!(restored_session, session);
    }

    #[test]
    fn snapshot_budget_expiry_and_malformed_bytes_are_rejected() {
        let mut store = RepetitionStore::new_location();
        alloc::rc::Rc::make_mut(&mut store.data)
            .exact
            .push("sensitive player line".to_string());
        let denied = store
            .snapshot_with_policy(SnapshotPolicy {
                max_logical_bytes: u64::MAX,
                pinned: true,
                expires_after_revisions: None,
                capture_sensitive: false,
            })
            .expect_err("sensitive capture requires consent");
        assert_eq!(
            denied.diagnostics()[0].code(),
            DiagnosticCode::SNAPSHOT_LIMIT
        );
        let snapshot = store
            .snapshot_with_policy(SnapshotPolicy {
                max_logical_bytes: u64::MAX,
                pinned: true,
                expires_after_revisions: Some(0),
                capture_sensitive: true,
            })
            .expect("consented snapshot");
        assert!(snapshot.pinned());
        let expired = RepetitionStore::restore_at(&snapshot, snapshot.revision() + 1)
            .expect_err("expired snapshot fails");
        assert_eq!(
            expired.diagnostics()[0].code(),
            DiagnosticCode::SNAPSHOT_EXPIRED
        );

        let mut bytes = snapshot.to_bytes();
        bytes.push(0);
        assert_eq!(
            RepetitionSnapshot::from_bytes(&bytes)
                .expect_err("trailing byte fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::SNAPSHOT
        );
    }
}
