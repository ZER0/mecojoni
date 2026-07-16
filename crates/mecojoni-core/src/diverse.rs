use alloc::{
    collections::{BTreeMap, VecDeque},
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    CompiledGrammar, DataBinding, Diagnostic, DiagnosticCode, GenerationLimits, GenerationRequest,
    GenerationResult, LocationProfile, MecoError, MecoResult, Severity, SplitMix64,
};

/// Compatibility identifier for transactional repetition-resistant sampling.
pub const DIVERSE_SAMPLER_VERSION: &str = "diverse/1";

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
pub struct DiverseGenerationRequest<'a> {
    pub entry: Option<&'a str>,
    pub limits: GenerationLimits,
    pub data: &'a [DataBinding],
    pub trace_bindings: bool,
    pub trace_selections: bool,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CountedHistory {
    capacity: usize,
    entries: VecDeque<String>,
    counts: CountMap,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FragmentHistory {
    capacity: usize,
    entries: VecDeque<Vec<String>>,
    counts: CountMap,
}

impl FragmentHistory {
    fn with_capacity(capacity: u32) -> Self {
        Self {
            capacity: usize::try_from(capacity).unwrap_or(usize::MAX),
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
        }
        self.entries.push_back(fragments);
        if self.entries.len() > self.capacity {
            for fragment in self
                .entries
                .pop_front()
                .expect("overfull fragment history has a head")
            {
                self.counts.decrement(&fragment);
            }
        }
    }
}

impl CountedHistory {
    fn with_capacity(capacity: u32) -> Self {
        Self {
            capacity: usize::try_from(capacity).unwrap_or(usize::MAX),
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
        self.entries.push_back(value);
        if self.entries.len() > self.capacity {
            let oldest = self
                .entries
                .pop_front()
                .expect("overfull history has a head");
            self.counts.decrement(&oldest);
        }
    }
}

/// Mutable repetition domain shared explicitly by one or more sampler sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepetitionStore {
    revision: u64,
    busy: bool,
    structural: BTreeMap<String, VecDeque<u32>>,
    exact: CountedHistory,
    edges: FragmentHistory,
}

impl Default for RepetitionStore {
    fn default() -> Self {
        Self::new_location()
    }
}

impl RepetitionStore {
    #[must_use]
    pub fn new_location() -> Self {
        let profile = LocationProfile::V1;
        Self {
            revision: 0,
            busy: false,
            structural: BTreeMap::new(),
            exact: CountedHistory::with_capacity(profile.exact_history_window),
            edges: FragmentHistory::with_capacity(profile.edge_history_window),
        }
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn exact_len(&self) -> usize {
        self.exact.entries.len()
    }

    #[must_use]
    pub fn edge_len(&self) -> usize {
        self.edges.entries.len()
    }

    pub(crate) fn structural_history(&self, rule: &str) -> Option<&VecDeque<u32>> {
        self.structural.get(rule)
    }

    pub(crate) fn exact_count(&self, text: &str) -> u32 {
        self.exact.count(&normalize_text(text))
    }

    pub(crate) fn edge_score(&self, text: &str) -> u64 {
        edge_fragments(text)
            .iter()
            .map(|fragment| u64::from(self.edges.count(fragment)))
            .sum()
    }

    fn commit(&mut self, selections: &[(String, u32)], text: &str) {
        let horizon =
            usize::try_from(LocationProfile::V1.soft_cooldown_horizon).unwrap_or(usize::MAX);
        for (rule, production) in selections {
            let history = self.structural.entry(rule.clone()).or_default();
            history.push_back(*production);
            while history.len() > horizon {
                history.pop_front();
            }
        }
        self.exact.push(normalize_text(text));
        self.edges.push(edge_fragments(text));
        self.revision = self.revision.wrapping_add(1);
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
        store.commit(&selections, generation.text());
        self.random = reserved;
        Ok(DiverseResult {
            generation,
            attempts,
            winner_attempt: ranking.2,
            exact_repetitions: ranking.0,
            edge_repetitions: ranking.1,
            committed_revision: store.revision,
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
}

pub(crate) struct DiverseCandidateState<'a> {
    store: &'a RepetitionStore,
    pub(crate) selections: Vec<(String, u32)>,
}

impl<'a> DiverseCandidateState<'a> {
    fn new(store: &'a RepetitionStore) -> Self {
        Self {
            store,
            selections: Vec::new(),
        }
    }

    pub(crate) fn recent(&self, rule: &str) -> Vec<u32> {
        self.store
            .structural_history(rule)
            .into_iter()
            .flat_map(|history| history.iter().copied())
            .chain(
                self.selections
                    .iter()
                    .filter(|(candidate, _)| candidate == rule)
                    .map(|(_, production)| *production),
            )
            .collect()
    }

    pub(crate) fn selection_age(&self, rule: &str, production: usize) -> Option<u32> {
        let production = u32::try_from(production).unwrap_or(u32::MAX);
        self.recent(rule)
            .iter()
            .rev()
            .position(|candidate| *candidate == production)
            .map(|age| u32::try_from(age + 1).unwrap_or(u32::MAX))
    }

    pub(crate) fn record(&mut self, rule: &str, production: usize) {
        self.selections.push((
            rule.to_string(),
            u32::try_from(production).unwrap_or(u32::MAX),
        ));
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

fn state_error(code: DiagnosticCode, message: &str) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{
        CountedHistory, DiverseGenerationRequest, FragmentHistory, RepetitionStore, SamplerSession,
        edge_fragments, normalize_text,
    };
    use crate::{
        DiagnosticCode, LocationProfile, PackageInput, PackageSource, SourceFile, SourceId,
        compile_package,
    };

    #[test]
    fn counted_history_evicts_without_shifting_and_retains_duplicate_counts() {
        let mut history = CountedHistory::with_capacity(2);
        history.push("a".to_string());
        history.push("a".to_string());
        history.push("b".to_string());
        assert_eq!(history.count("a"), 1);
        assert_eq!(history.count("b"), 1);
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
        let mut history = CountedHistory::with_capacity(LocationProfile::V1.exact_history_window);
        for index in 0..=LocationProfile::V1.exact_history_window {
            history.push(index.to_string());
        }
        assert_eq!(history.entries.len(), 50_000);
        assert_eq!(history.count("0"), 0);
        assert_eq!(history.count("1"), 1);
    }

    #[test]
    fn edge_window_counts_phrases_and_evicts_all_of_their_fragments() {
        let mut history = FragmentHistory::with_capacity(2);
        history.push(alloc::vec!["a".to_string(), "shared".to_string()]);
        history.push(alloc::vec!["b".to_string(), "shared".to_string()]);
        history.push(alloc::vec!["c".to_string()]);
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.count("a"), 0);
        assert_eq!(history.count("shared"), 1);

        let mut location = FragmentHistory::with_capacity(LocationProfile::V1.edge_history_window);
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
}
