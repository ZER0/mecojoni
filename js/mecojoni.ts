const WIRE_VERSION = 1;
const OP_PACKAGE_CREATE = 1;
const OP_COMPILE = 2;
const OP_GENERATE_TYPED = 4;
const OP_COMPILE_WITH_MANIFEST = 5;
const OP_GENERATE_STRUCTURAL = 6;
const OP_REPETITION_CREATE = 7;
const OP_SESSION_CREATE = 8;
const OP_GENERATE_DIVERSE = 9;
const OP_SESSION_SNAPSHOT_EXPORT = 10;
const OP_SESSION_SNAPSHOT_IMPORT = 11;
const OP_REPETITION_SNAPSHOT_EXPORT = 12;
const OP_REPETITION_SNAPSHOT_IMPORT = 13;
const OP_ARTIFACT_LOAD = 14;
const OP_ARTIFACT_INSPECT = 15;
const OP_EMBEDDED_GRAMMAR_OPEN = 16;

const PAYLOAD_ERROR = 0;
const PAYLOAD_PACKAGE = 1;
const PAYLOAD_COMPILE = 2;
const PAYLOAD_GENERATE = 3;
const PAYLOAD_STRUCTURAL = 4;
const PAYLOAD_DIVERSE = 5;
const PAYLOAD_SNAPSHOT = 6;
const PAYLOAD_ARTIFACT = 7;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("utf-8", { fatal: true });

interface WasmExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  meco_abi_version(): number;
  meco_core_api_version(): number;
  meco_alloc(length: number, alignment: number): number;
  meco_dealloc(pointer: number, length: number, alignment: number): void;
  meco_call(operation: number, pointer: number, length: number): number;
  meco_result_status(result: number): number;
  meco_result_value_handle(result: number): number;
  meco_result_payload_length(result: number): number;
  meco_result_payload_copy(result: number, destination: number, capacity: number): number;
  meco_handle_dispose(handle: number): void;
  meco_live_handle_count(): number;
  meco_live_allocation_count(): number;
  meco_live_allocation_bytes(): number;
}

export interface ResolvedImport {
  authoredPath: string;
  targetId: string;
}

export interface PackageModule {
  canonicalId: string;
  sourceId: number;
  sourceName: string;
  source: string;
  resolvedImports: ResolvedImport[];
}

export interface PackageDescription {
  rootId: string;
  modules: PackageModule[];
}

export interface SourcePosition {
  byte: bigint;
  scalar: bigint;
}

export interface SourceSpan {
  sourceId: number;
  start: SourcePosition;
  end: SourcePosition;
}

export interface MecoDiagnostic {
  code: string;
  severity: "error" | "warning";
  span?: SourceSpan;
  message: string;
}

export interface MecoError {
  message: string;
  diagnostics: MecoDiagnostic[];
}

export type MecoResult<T> =
  | { ok: true; value: T; diagnostics: MecoDiagnostic[] }
  | { ok: false; error: MecoError; diagnostics: MecoDiagnostic[] };

export interface CompileSummary {
  entries: string[];
  defaultEntry?: string;
}

export interface ArtifactLimitOptions {
  maximumBytes?: number;
}

export interface ArtifactMetadata {
  version: string;
  debugProfile: "full" | "mapped" | "stripped";
  semanticPackageHash: bigint;
  bytecodeContentHash: bigint;
  totalBytes: bigint;
  ruleCount: number;
  productionCount: number;
  entries: readonly string[];
  defaultEntry?: string;
}

export type SchemaType =
  | { kind: "text" }
  | { kind: "number" }
  | { kind: "boolean" }
  | { kind: "enum"; name: string };

export interface MessageArgumentSchema {
  name: string;
  type: SchemaType;
}

export interface MessageSchema {
  id: string;
  arguments: readonly MessageArgumentSchema[];
}

export interface MessageManifest {
  messages: readonly MessageSchema[];
}

export interface FormatterRequest {
  messageId: string;
  arguments: Readonly<Record<string, MecoValue>>;
  requestedLocale: string;
  fallbackLocales: readonly string[];
}

export interface FormatterResponse {
  text: string;
  actualLocale: string;
  environmentHash: string;
  diagnostics?: readonly MecoDiagnostic[];
  workUnits: number;
  replayable: boolean;
}

export type MecoFormatter = (request: FormatterRequest) => FormatterResponse;

export interface MessageOutput {
  id: string;
  requestedLocale: string;
  actualLocale: string;
  environmentHash: string;
  workUnits: number;
  replayable: boolean;
}

export interface GenerationOutput {
  text: string;
  entry: string;
  expansions: number;
  samplerWords: number;
  bindings: BindingOutput[];
  selections: SelectionOutput[];
  provenance: ProvenanceOutput[];
  formatterDiagnostics: MecoDiagnostic[];
  message?: MessageOutput;
}

export interface DiverseOutput extends GenerationOutput {
  attempts: number;
  winnerAttempt: number;
  exactRepetitions: number;
  edgeRepetitions: bigint;
  committedRevision: bigint;
  receipt: ReplayReceiptOutput;
}

export type ProvenanceKind =
  | "production"
  | "authoredText"
  | "hostValue"
  | "boundValue"
  | "emittingCapture"
  | "binding"
  | "message";

export interface OutputRange {
  startByte: bigint;
  endByte: bigint;
  startScalar: bigint;
  endScalar: bigint;
}

export interface ProvenanceOutput {
  id: number;
  parent?: number;
  kind: ProvenanceKind;
  rule: string;
  productionId: string;
  sourceSpan: SourceSpan;
  output?: OutputRange;
  depth: number;
  name?: string;
}

export interface ReplayReceiptOutput {
  version: number;
  grammarHash: bigint;
  samplerVersion: string;
  normalizerVersion: string;
  tokenizerVersion: string;
  preSessionHash: bigint;
  preSessionWords: bigint;
  preRepetitionHash: bigint;
  preRepetitionRevision: bigint;
  reservedWords: bigint;
  requestDigest: bigint;
  effectiveEntry: string;
  winnerAttempt: number;
  derivationHash: bigint;
  finalTextHash: bigint;
  postSessionHash: bigint;
  postRepetitionHash: bigint;
  postRepetitionRevision: bigint;
}

export type MecoValue =
  | { kind: "text"; value: string }
  | { kind: "number"; numerator: bigint; denominator: bigint }
  | { kind: "boolean"; value: boolean }
  | { kind: "enum"; value: string };

export interface BindingOutput {
  name: string;
  emitted: boolean;
  value: MecoValue;
}

export interface EligibleWeightOutput {
  production: number;
  productionId: string;
  baseWeight: { numerator: bigint; denominator: bigint };
  normalizedWeight: bigint;
}

export interface SelectionOutput {
  rule: string;
  selectedProduction: number;
  selectedProductionId: string;
  eligible: EligibleWeightOutput[];
}

export interface GenerationLimitOptions {
  maxDepth: number;
  maxExpansions: number;
  maxOutputScalars: number;
  maxOutputBytes: number;
  maxSamplerWords: number;
}

export interface GenerationOptions {
  entry?: string;
  seed: bigint;
  limits?: Partial<GenerationLimitOptions>;
  data?: Readonly<Record<string, MecoValue>>;
  traceBindings?: boolean;
  traceSelections?: boolean;
  traceProvenance?: boolean;
  locale?: string;
  fallbackLocales?: readonly string[];
  formatter?: MecoFormatter;
}

export interface DiverseOptions {
  entry?: string;
  limits?: Partial<GenerationLimitOptions>;
  data?: Readonly<Record<string, MecoValue>>;
  traceBindings?: boolean;
  traceSelections?: boolean;
  traceProvenance?: boolean;
  cancelled?: boolean;
}

const DEFAULT_LIMITS: GenerationLimitOptions = {
  maxDepth: 80,
  maxExpansions: 2_000,
  maxOutputScalars: 16_384,
  maxOutputBytes: 65_536,
  maxSamplerWords: 8_192,
};

abstract class OwnedHandle {
  #owner: Mecojoni;
  #handle: number;

  protected constructor(owner: Mecojoni, handle: number) {
    this.#owner = owner;
    this.#handle = handle;
  }

  get owner(): Mecojoni {
    return this.#owner;
  }

  get handle(): number {
    if (this.#handle === 0) {
      throw new Error("Mecojoni handle is already disposed");
    }
    return this.#handle;
  }

  dispose(): void {
    if (this.#handle !== 0) {
      this.#owner.disposeHandle(this.#handle);
      this.#handle = 0;
    }
  }
}

export class MecoPackage extends OwnedHandle {
  constructor(owner: Mecojoni, handle: number) {
    super(owner, handle);
  }
}

export class CompiledGrammar extends OwnedHandle {
  readonly entries: readonly string[];
  readonly defaultEntry?: string;

  constructor(owner: Mecojoni, handle: number, summary: CompileSummary) {
    super(owner, handle);
    this.entries = Object.freeze([...summary.entries]);
    this.defaultEntry = summary.defaultEntry;
  }
}

export class RepetitionStore extends OwnedHandle {
  constructor(owner: Mecojoni, handle: number) {
    super(owner, handle);
  }

  snapshot(): MecoResult<Uint8Array> {
    return this.owner.exportRepetitionSnapshot(this);
  }
}

export class SamplerSession extends OwnedHandle {
  constructor(owner: Mecojoni, handle: number) {
    super(owner, handle);
  }

  snapshot(): MecoResult<Uint8Array> {
    return this.owner.exportSessionSnapshot(this);
  }
}

interface DecodedResult {
  status: number;
  valueHandle: number;
  kind: number;
  reader: Reader;
}

export class Mecojoni {
  #exports: WasmExports;

  private constructor(exports: WasmExports) {
    this.#exports = exports;
  }

  static async instantiate(bytes: BufferSource): Promise<Mecojoni> {
    const instantiated = await WebAssembly.instantiate(bytes, {});
    const instance = instantiated instanceof WebAssembly.Instance
      ? instantiated
      : instantiated.instance;
    const exports = instance.exports as WasmExports;
    if (!(exports.memory instanceof WebAssembly.Memory)) {
      throw new Error("Mecojoni WASM does not export linear memory");
    }
    const meco = new Mecojoni(exports);
    if (meco.abiVersion !== 1) {
      throw new Error(`Unsupported Mecojoni ABI ${meco.abiVersion}`);
    }
    return meco;
  }

  get abiVersion(): number {
    return this.#exports.meco_abi_version();
  }

  get coreApiVersion(): number {
    return this.#exports.meco_core_api_version();
  }

  get liveHandleCount(): number {
    return this.#exports.meco_live_handle_count();
  }

  /** Outstanding host-visible ABI buffers; wrapper calls return this to zero. */
  get liveAllocationCount(): number {
    return this.#exports.meco_live_allocation_count();
  }

  /** Logical bytes in outstanding host-visible ABI buffers. */
  get liveAllocationBytes(): number {
    return this.#exports.meco_live_allocation_bytes();
  }

  /** Current high-water-capable WebAssembly linear-memory size in bytes. */
  get linearMemoryBytes(): number {
    return this.#exports.memory.buffer.byteLength;
  }

  createPackage(description: PackageDescription): MecoResult<MecoPackage> {
    let decoded: DecodedResult | undefined;
    try {
      decoded = this.invoke(OP_PACKAGE_CREATE, encodePackage(description));
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_PACKAGE);
      decoded.reader.finish();
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "package handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      return {
        ok: true,
        value: new MecoPackage(this, handle),
        diagnostics: [],
      };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  compile(
    mecoPackage: MecoPackage,
    manifest?: MessageManifest,
  ): MecoResult<CompiledGrammar> {
    let decoded: DecodedResult | undefined;
    try {
      this.assertOwner(mecoPackage);
      const writer = request();
      writer.u32(mecoPackage.handle);
      if (manifest !== undefined) writer.manifest(manifest);
      decoded = this.invoke(
        manifest === undefined ? OP_COMPILE : OP_COMPILE_WITH_MANIFEST,
        writer.finish(),
      );
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_COMPILE);
      const reader = decoded.reader;
      const entryCount = reader.u32();
      const entries = Array.from({ length: entryCount }, () => reader.string());
      const defaultEntry = reader.optionalString();
      const diagnostics = reader.diagnostics();
      reader.finish();
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "grammar handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      const summary = { entries, defaultEntry };
      return {
        ok: true,
        value: new CompiledGrammar(this, handle, summary),
        diagnostics,
      };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  compilePackage(
    description: PackageDescription,
    manifest?: MessageManifest,
  ): MecoResult<CompiledGrammar> {
    const created = this.createPackage(description);
    if (!created.ok) return created;
    try {
      return this.compile(created.value, manifest);
    } finally {
      created.value.dispose();
    }
  }

  loadArtifact(
    bytes: Uint8Array,
    limits: ArtifactLimitOptions = {},
  ): MecoResult<CompiledGrammar> {
    let decoded: DecodedResult | undefined;
    try {
      validateArtifactInput(bytes, limits);
      decoded = this.invoke(OP_ARTIFACT_LOAD, bytes);
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_COMPILE);
      const entries = Array.from({ length: decoded.reader.u32() }, () => decoded!.reader.string());
      const defaultEntry = decoded.reader.optionalString();
      const diagnostics = decoded.reader.diagnostics();
      decoded.reader.finish();
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "grammar handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      return {
        ok: true,
        value: new CompiledGrammar(this, handle, { entries, defaultEntry }),
        diagnostics,
      };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  inspectArtifact(
    bytes: Uint8Array,
    limits: ArtifactLimitOptions = {},
  ): MecoResult<ArtifactMetadata> {
    try {
      validateArtifactInput(bytes, limits);
      const decoded = this.invoke(OP_ARTIFACT_INSPECT, bytes);
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_ARTIFACT);
      const profiles = ["full", "mapped", "stripped"] as const;
      const version = decoded.reader.string();
      const debugProfile = profiles[decoded.reader.u8()];
      if (debugProfile === undefined) throw new Error("Invalid artifact debug profile");
      const semanticPackageHash = decoded.reader.u64();
      const bytecodeContentHash = decoded.reader.u64();
      const totalBytes = decoded.reader.u64();
      const ruleCount = decoded.reader.u32();
      const productionCount = decoded.reader.u32();
      const entries = Array.from({ length: decoded.reader.u32() }, () => decoded.reader.string());
      const defaultEntry = decoded.reader.optionalString();
      decoded.reader.finish();
      return {
        ok: true,
        value: {
          version,
          debugProfile,
          semanticPackageHash,
          bytecodeContentHash,
          totalBytes,
          ruleCount,
          productionCount,
          entries,
          defaultEntry,
        },
        diagnostics: [],
      };
    } catch (error) {
      return caughtFailure(error);
    }
  }

  openEmbeddedGrammar(): MecoResult<CompiledGrammar> {
    let decoded: DecodedResult | undefined;
    try {
      decoded = this.invoke(OP_EMBEDDED_GRAMMAR_OPEN, request().finish());
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_COMPILE);
      const entries = Array.from({ length: decoded.reader.u32() }, () => decoded!.reader.string());
      const defaultEntry = decoded.reader.optionalString();
      const diagnostics = decoded.reader.diagnostics();
      decoded.reader.finish();
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "grammar handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      return {
        ok: true,
        value: new CompiledGrammar(this, handle, { entries, defaultEntry }),
        diagnostics,
      };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  createRepetitionStore(): MecoResult<RepetitionStore> {
    return this.createStateHandle(
      OP_REPETITION_CREATE,
      request().finish(),
      (handle) => new RepetitionStore(this, handle),
    );
  }

  createSession(seed: bigint): MecoResult<SamplerSession> {
    try {
      validateSeed(seed);
      const writer = request();
      writer.u64(seed);
      return this.createStateHandle(
        OP_SESSION_CREATE,
        writer.finish(),
        (handle) => new SamplerSession(this, handle),
      );
    } catch (error) {
      return caughtFailure(error);
    }
  }

  exportSessionSnapshot(session: SamplerSession): MecoResult<Uint8Array> {
    return this.exportStateSnapshot(OP_SESSION_SNAPSHOT_EXPORT, session);
  }

  restoreSessionSnapshot(snapshot: Uint8Array): MecoResult<SamplerSession> {
    return this.importStateSnapshot(
      OP_SESSION_SNAPSHOT_IMPORT,
      snapshot,
      (handle) => new SamplerSession(this, handle),
    );
  }

  exportRepetitionSnapshot(repetition: RepetitionStore): MecoResult<Uint8Array> {
    return this.exportStateSnapshot(OP_REPETITION_SNAPSHOT_EXPORT, repetition);
  }

  restoreRepetitionSnapshot(snapshot: Uint8Array): MecoResult<RepetitionStore> {
    return this.importStateSnapshot(
      OP_REPETITION_SNAPSHOT_IMPORT,
      snapshot,
      (handle) => new RepetitionStore(this, handle),
    );
  }

  generateDiverse(
    grammar: CompiledGrammar,
    session: SamplerSession,
    repetition: RepetitionStore,
    options: DiverseOptions,
  ): MecoResult<DiverseOutput> {
    try {
      this.assertOwner(grammar);
      this.assertOwner(session);
      this.assertOwner(repetition);
      const limits = { ...DEFAULT_LIMITS, ...options.limits };
      for (const [name, value] of Object.entries(limits)) validateU32(value, name);
      const writer = request();
      writer.u32(grammar.handle);
      writer.u64(0n); // Reserved in meco-wire/1; session state is the sole random source.
      writer.optionalString(options.entry);
      writer.u32(limits.maxDepth);
      writer.u32(limits.maxExpansions);
      writer.u32(limits.maxOutputScalars);
      writer.u32(limits.maxOutputBytes);
      writer.u32(limits.maxSamplerWords);
      writer.u8(options.traceBindings === true ? 1 : 0);
      writer.u8(options.traceSelections === true ? 1 : 0);
      writer.u8(options.traceProvenance === true ? 1 : 0);
      const data = Object.entries(options.data ?? {}).sort(([left], [right]) =>
        left < right ? -1 : left > right ? 1 : 0
      );
      writer.u32(data.length);
      for (const [name, value] of data) {
        writer.string(name);
        writer.value(value);
      }
      writer.u32(session.handle);
      writer.u32(repetition.handle);
      writer.u8(options.cancelled === true ? 1 : 0);
      const decoded = this.invoke(OP_GENERATE_DIVERSE, writer.finish());
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_DIVERSE);
      const value: DiverseOutput = {
        text: decoded.reader.string(),
        entry: decoded.reader.string(),
        expansions: decoded.reader.u32(),
        samplerWords: decoded.reader.u32(),
        ...readTraces(decoded.reader),
        formatterDiagnostics: [],
        attempts: decoded.reader.u32(),
        winnerAttempt: decoded.reader.u32(),
        exactRepetitions: decoded.reader.u32(),
        edgeRepetitions: decoded.reader.u64(),
        committedRevision: decoded.reader.u64(),
        receipt: readReplayReceipt(decoded.reader),
      };
      decoded.reader.finish();
      return { ok: true, value, diagnostics: [] };
    } catch (error) {
      return caughtFailure(error);
    }
  }

  generateWeighted(
    grammar: CompiledGrammar,
    options: GenerationOptions,
  ): MecoResult<GenerationOutput> {
    try {
      this.assertOwner(grammar);
      const limits = { ...DEFAULT_LIMITS, ...options.limits };
      validateSeed(options.seed);
      for (const [name, value] of Object.entries(limits)) validateU32(value, name);
      const writer = request();
      writer.u32(grammar.handle);
      writer.u64(options.seed);
      writer.optionalString(options.entry);
      writer.u32(limits.maxDepth);
      writer.u32(limits.maxExpansions);
      writer.u32(limits.maxOutputScalars);
      writer.u32(limits.maxOutputBytes);
      writer.u32(limits.maxSamplerWords);
      writer.u8(options.traceBindings === true ? 1 : 0);
      writer.u8(options.traceSelections === true ? 1 : 0);
      writer.u8(options.traceProvenance === true ? 1 : 0);
      const data = Object.entries(options.data ?? {}).sort(([left], [right]) =>
        left < right ? -1 : left > right ? 1 : 0
      );
      writer.u32(data.length);
      for (const [name, value] of data) {
        writer.string(name);
        writer.value(value);
      }
      const localized = options.formatter !== undefined || options.locale !== undefined ||
        (options.fallbackLocales?.length ?? 0) !== 0;
      if (localized) {
        if (options.formatter === undefined) {
          return localFailure("E_FORMATTER_REQUIRED", "localized generation requires a formatter");
        }
        if (options.locale === undefined) {
          return localFailure("E_LOCALE", "localized generation requires an explicit locale");
        }
        writer.string(options.locale);
        writer.u32(options.fallbackLocales?.length ?? 0);
        for (const fallback of options.fallbackLocales ?? []) writer.string(fallback);
      }
      const decoded = this.invoke(
        localized ? OP_GENERATE_STRUCTURAL : OP_GENERATE_TYPED,
        writer.finish(),
      );
      if (decoded.status !== 0) return decodeFailure(decoded);
      if (!localized) {
        expectKind(decoded, PAYLOAD_GENERATE);
        const value = {
          text: decoded.reader.string(),
          entry: decoded.reader.string(),
          expansions: decoded.reader.u32(),
          samplerWords: decoded.reader.u32(),
          ...readTraces(decoded.reader),
          formatterDiagnostics: [],
        };
        decoded.reader.finish();
        return { ok: true, value, diagnostics: [] };
      }

      expectKind(decoded, PAYLOAD_STRUCTURAL);
      const contentKind = decoded.reader.u8();
      let text: string | undefined;
      let formatterRequest: FormatterRequest | undefined;
      if (contentKind === 0) {
        text = decoded.reader.string();
      } else if (contentKind === 1) {
        const messageId = decoded.reader.string();
        const argumentCount = decoded.reader.u32();
        const argumentEntries = Array.from(
          { length: argumentCount },
          () => [decoded.reader.string(), Object.freeze(decoded.reader.value())] as const,
        );
        formatterRequest = {
          messageId,
          arguments: Object.freeze(Object.fromEntries(argumentEntries)),
          requestedLocale: decoded.reader.string(),
          fallbackLocales: Object.freeze(
            Array.from({ length: decoded.reader.u32() }, () => decoded.reader.string()),
          ),
        };
      } else {
        throw new Error(`Invalid structural content kind ${contentKind}`);
      }
      const structural = {
        entry: decoded.reader.string(),
        expansions: decoded.reader.u32(),
        samplerWords: decoded.reader.u32(),
        ...readTraces(decoded.reader),
      };
      decoded.reader.finish();
      if (formatterRequest === undefined) {
        return {
          ok: true,
          value: { text: text ?? "", ...structural, formatterDiagnostics: [] },
          diagnostics: [],
        };
      }
      let formatted: FormatterResponse;
      try {
        formatted = options.formatter!(formatterRequest);
      } catch (error) {
        return localFailure(
          "E_FORMATTER",
          error instanceof Error ? error.message : String(error),
        );
      }
      if (isPromiseLike(formatted)) {
        return localFailure("E_FORMATTER", "formatter callback must be synchronous");
      }
      const formatterFailure = validateFormatterResponse(formatterRequest, formatted, limits);
      if (formatterFailure !== undefined) return formatterFailure;
      const formatterDiagnostics = [...(formatted.diagnostics ?? [])];
      const value: GenerationOutput = {
        text: formatted.text,
        ...structural,
        provenance: finalizeFormattedProvenance(structural.provenance, formatted.text),
        formatterDiagnostics,
        message: {
          id: formatterRequest.messageId,
          requestedLocale: formatterRequest.requestedLocale,
          actualLocale: formatted.actualLocale,
          environmentHash: formatted.environmentHash,
          workUnits: formatted.workUnits,
          replayable: formatted.replayable,
        },
      };
      return { ok: true, value, diagnostics: formatterDiagnostics };
    } catch (error) {
      return caughtFailure(error);
    }
  }

  disposeHandle(handle: number): void {
    this.#exports.meco_handle_dispose(handle);
  }

  private assertOwner(handle: OwnedHandle): void {
    if (handle.owner !== this) throw new Error("Mecojoni handle belongs to another WASM instance");
  }

  private exportStateSnapshot(operation: number, handle: OwnedHandle): MecoResult<Uint8Array> {
    try {
      this.assertOwner(handle);
      const writer = request();
      writer.u32(handle.handle);
      const decoded = this.invoke(operation, writer.finish());
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_SNAPSHOT);
      const snapshot = decoded.reader.bytes();
      decoded.reader.finish();
      return { ok: true, value: snapshot, diagnostics: [] };
    } catch (error) {
      return caughtFailure(error);
    }
  }

  private importStateSnapshot<T extends OwnedHandle>(
    operation: number,
    snapshot: Uint8Array,
    create: (handle: number) => T,
  ): MecoResult<T> {
    let decoded: DecodedResult | undefined;
    try {
      const writer = request();
      writer.bytes(snapshot);
      decoded = this.invoke(operation, writer.finish());
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_SNAPSHOT);
      const canonical = decoded.reader.bytes();
      decoded.reader.finish();
      if (
        canonical.length !== snapshot.length ||
        canonical.some((byte, index) => byte !== snapshot[index])
      ) {
        throw new Error("restored snapshot bytes were not canonical");
      }
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "state handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      return { ok: true, value: create(handle), diagnostics: [] };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  private createStateHandle<T extends OwnedHandle>(
    operation: number,
    input: Uint8Array,
    create: (handle: number) => T,
  ): MecoResult<T> {
    let decoded: DecodedResult | undefined;
    try {
      decoded = this.invoke(operation, input);
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_PACKAGE);
      decoded.reader.finish();
      if (decoded.valueHandle === 0) return localFailure("E_ABI_VALUE", "state handle is absent");
      const handle = decoded.valueHandle;
      decoded.valueHandle = 0;
      return { ok: true, value: create(handle), diagnostics: [] };
    } catch (error) {
      if (decoded?.valueHandle) this.disposeHandle(decoded.valueHandle);
      return caughtFailure(error);
    }
  }

  private invoke(operation: number, input: Uint8Array): DecodedResult {
    const inputPointer = this.allocate(input.length);
    let resultHandle = 0;
    try {
      this.memoryBytes().set(input, inputPointer);
      resultHandle = this.#exports.meco_call(operation, inputPointer, input.length);
    } finally {
      this.#exports.meco_dealloc(inputPointer, input.length, 1);
    }
    if (resultHandle === 0) throw new Error("Mecojoni ABI call returned no result handle");
    let valueHandle = 0;
    try {
      const status = this.#exports.meco_result_status(resultHandle);
      if (status === 2) throw new Error("Mecojoni ABI returned an invalid result handle");
      valueHandle = this.#exports.meco_result_value_handle(resultHandle);
      const payloadLength = this.#exports.meco_result_payload_length(resultHandle);
      const payload = this.copyPayload(resultHandle, payloadLength);
      const reader = new Reader(payload);
      const version = reader.u32();
      if (version !== WIRE_VERSION) throw new Error(`Unsupported response wire version ${version}`);
      return { status, valueHandle, kind: reader.u32(), reader };
    } catch (error) {
      if (valueHandle !== 0) this.#exports.meco_handle_dispose(valueHandle);
      throw error;
    } finally {
      this.#exports.meco_handle_dispose(resultHandle);
    }
  }

  private copyPayload(result: number, length: number): Uint8Array {
    if (length === 0) return new Uint8Array();
    const pointer = this.allocate(length);
    try {
      const required = this.#exports.meco_result_payload_copy(result, pointer, length);
      if (required !== length) throw new Error(`Payload copy expected ${length}, got ${required}`);
      return this.memoryBytes().slice(pointer, pointer + length);
    } finally {
      this.#exports.meco_dealloc(pointer, length, 1);
    }
  }

  private allocate(length: number): number {
    const pointer = this.#exports.meco_alloc(length, 1);
    if (pointer === 0) throw new Error(`Mecojoni failed to allocate ${length} bytes`);
    return pointer;
  }

  private memoryBytes(): Uint8Array {
    return new Uint8Array(this.#exports.memory.buffer);
  }
}

function finalizeFormattedProvenance(
  provenance: ProvenanceOutput[],
  text: string,
): ProvenanceOutput[] {
  const full: OutputRange = {
    startByte: 0n,
    endByte: BigInt(encodeStrict(text).length),
    startScalar: 0n,
    endScalar: BigInt(Array.from(text).length),
  };
  return provenance.map((node) => {
    const formattedAncestor = node.kind === "production" && node.output !== undefined &&
      node.output.startScalar === node.output.endScalar;
    return node.kind === "message" || formattedAncestor ? { ...node, output: full } : node;
  });
}

function encodePackage(description: PackageDescription): Uint8Array {
  const writer = request();
  writer.string(description.rootId);
  writer.u32(description.modules.length);
  for (const module of description.modules) {
    validateU32(module.sourceId, "sourceId");
    writer.string(module.canonicalId);
    writer.u32(module.sourceId);
    writer.string(module.sourceName);
    writer.bytes(encodeStrict(module.source));
    writer.u32(module.resolvedImports.length);
    for (const resolution of module.resolvedImports) {
      writer.string(resolution.authoredPath);
      writer.string(resolution.targetId);
    }
  }
  return writer.finish();
}

function validateArtifactInput(bytes: Uint8Array, limits: ArtifactLimitOptions): void {
  if (!(bytes instanceof Uint8Array)) throw new TypeError("artifact must be a Uint8Array");
  const maximumBytes = limits.maximumBytes ?? 64 * 1024 * 1024;
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0 || maximumBytes > 64 * 1024 * 1024) {
    throw new RangeError("maximumBytes must be an integer from 0 through 67108864");
  }
  if (bytes.byteLength > maximumBytes) {
    throw new RangeError(`artifact exceeds maximumBytes ${maximumBytes}`);
  }
}

function readTraces(reader: Reader): {
  bindings: BindingOutput[];
  selections: SelectionOutput[];
  provenance: ProvenanceOutput[];
} {
  const bindings = Array.from({ length: reader.u32() }, () => ({
    name: reader.string(),
    emitted: decodeBoolean(reader.u8(), "binding emitted flag"),
    value: reader.value(),
  }));
  const selections = Array.from({ length: reader.u32() }, () => ({
    rule: reader.string(),
    selectedProduction: reader.u32(),
    selectedProductionId: reader.string(),
    eligible: Array.from({ length: reader.u32() }, () => ({
      production: reader.u32(),
      productionId: reader.string(),
      baseWeight: {
        numerator: reader.i64(),
        denominator: reader.u64(),
      },
      normalizedWeight: reader.u64(),
    })),
  }));
  const kinds: ProvenanceKind[] = [
    "production",
    "authoredText",
    "hostValue",
    "boundValue",
    "emittingCapture",
    "binding",
    "message",
  ];
  const provenance = Array.from({ length: reader.u32() }, (): ProvenanceOutput => {
    const id = reader.u32();
    const hasParent = reader.u8();
    const parent = hasParent === 1 ? reader.u32() : undefined;
    if (hasParent > 1) throw new Error("Invalid provenance parent flag");
    const kind = kinds[reader.u8()];
    if (kind === undefined) throw new Error("Invalid provenance kind");
    const rule = reader.string();
    const productionId = reader.string();
    const sourceSpan = readSourceSpan(reader);
    const hasOutput = reader.u8();
    const output = hasOutput === 1
      ? {
        startByte: reader.u64(),
        endByte: reader.u64(),
        startScalar: reader.u64(),
        endScalar: reader.u64(),
      }
      : undefined;
    if (hasOutput > 1) throw new Error("Invalid provenance output flag");
    const depth = reader.u32();
    const hasName = reader.u8();
    const name = hasName === 1 ? reader.string() : undefined;
    if (hasName > 1) throw new Error("Invalid provenance name flag");
    return { id, parent, kind, rule, productionId, sourceSpan, output, depth, name };
  });
  return { bindings, selections, provenance };
}

function readSourceSpan(reader: Reader): SourceSpan {
  return {
    sourceId: reader.u32(),
    start: { byte: reader.u64(), scalar: reader.u64() },
    end: { byte: reader.u64(), scalar: reader.u64() },
  };
}

function readReplayReceipt(reader: Reader): ReplayReceiptOutput {
  return {
    version: reader.u32(),
    grammarHash: reader.u64(),
    samplerVersion: reader.string(),
    normalizerVersion: reader.string(),
    tokenizerVersion: reader.string(),
    preSessionHash: reader.u64(),
    preSessionWords: reader.u64(),
    preRepetitionHash: reader.u64(),
    preRepetitionRevision: reader.u64(),
    reservedWords: reader.u64(),
    requestDigest: reader.u64(),
    effectiveEntry: reader.string(),
    winnerAttempt: reader.u32(),
    derivationHash: reader.u64(),
    finalTextHash: reader.u64(),
    postSessionHash: reader.u64(),
    postRepetitionHash: reader.u64(),
    postRepetitionRevision: reader.u64(),
  };
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return typeof value === "object" && value !== null && "then" in value &&
    typeof (value as { then?: unknown }).then === "function";
}

function validateFormatterResponse(
  request: FormatterRequest,
  response: FormatterResponse,
  limits: GenerationLimitOptions,
): MecoResult<GenerationOutput> | undefined {
  if (typeof response !== "object" || response === null) {
    return localFailure("E_FORMATTER", "formatter must return a result object");
  }
  if (typeof response.text !== "string") {
    return localFailure("E_FORMATTER", "formatter result text must be a string");
  }
  let bytes: number;
  try {
    bytes = encodeStrict(response.text).length;
  } catch (error) {
    return localFailure(
      "E_FORMATTER",
      error instanceof Error ? error.message : String(error),
    );
  }
  const scalars = Array.from(response.text).length;
  if (scalars > limits.maxOutputScalars || bytes > limits.maxOutputBytes) {
    return localFailure(
      "E_LIMIT_OUTPUT",
      `formatted output exceeds scalar/byte limits (${scalars}/${bytes})`,
    );
  }
  if (
    typeof response.actualLocale !== "string" ||
    ![request.requestedLocale, ...request.fallbackLocales].includes(response.actualLocale)
  ) {
    return localFailure(
      "E_LOCALE",
      "formatter actualLocale is outside the requested fallback chain",
    );
  }
  if (!Number.isSafeInteger(response.workUnits) || response.workUnits < 0) {
    return localFailure("E_FORMATTER", "formatter workUnits must be a non-negative integer");
  }
  if (response.workUnits > 10_000) {
    return localFailure("E_FORMATTER_LIMIT", "formatter workUnits exceed 10000");
  }
  if (typeof response.replayable !== "boolean") {
    return localFailure("E_FORMATTER", "formatter replayable must be boolean");
  }
  if (typeof response.environmentHash !== "string") {
    return localFailure("E_FORMATTER", "formatter environmentHash must be a string");
  }
  if (response.replayable && response.environmentHash.length === 0) {
    return localFailure(
      "E_FORMATTER",
      "a replayable formatter requires a non-empty environmentHash",
    );
  }
  if (response.diagnostics !== undefined && !Array.isArray(response.diagnostics)) {
    return localFailure("E_FORMATTER", "formatter diagnostics must be an array");
  }
  const diagnostics = [...(response.diagnostics ?? [])];
  if (diagnostics.some((diagnostic) => diagnostic.severity === "error")) {
    const primary: MecoDiagnostic = {
      code: "E_FORMATTER",
      severity: "error",
      message: "formatter reported a fatal diagnostic",
    };
    return {
      ok: false,
      error: { message: primary.message, diagnostics: [primary, ...diagnostics] },
      diagnostics: [primary, ...diagnostics],
    };
  }
  return undefined;
}

function request(): Writer {
  const writer = new Writer();
  writer.u32(WIRE_VERSION);
  return writer;
}

function decodeFailure<T>(decoded: DecodedResult): MecoResult<T> {
  expectKind(decoded, PAYLOAD_ERROR);
  const diagnostics = decoded.reader.diagnostics();
  decoded.reader.finish();
  const message = diagnostics[0]?.message ?? "Unknown Mecojoni error";
  return { ok: false, error: { message, diagnostics }, diagnostics };
}

function expectKind(decoded: DecodedResult, expected: number): void {
  if (decoded.kind !== expected) {
    throw new Error(`Payload kind ${decoded.kind}, expected ${expected}`);
  }
}

function caughtFailure<T>(error: unknown): MecoResult<T> {
  return localFailure("E_JS_BOUNDARY", error instanceof Error ? error.message : String(error));
}

function localFailure<T>(code: string, message: string): MecoResult<T> {
  const diagnostics: MecoDiagnostic[] = [{ code, severity: "error", message }];
  return { ok: false, error: { message, diagnostics }, diagnostics };
}

function validateSeed(seed: bigint): void {
  if (seed < 0n || seed > 0xffff_ffff_ffff_ffffn) throw new RangeError("seed must be a u64 bigint");
}

function validateU32(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new RangeError(`${name} must be a u32 safe integer`);
  }
}

function decodeBoolean(value: number, name: string): boolean {
  if (value === 0) return false;
  if (value === 1) return true;
  throw new Error(`Invalid ${name}`);
}

function encodeStrict(value: string): Uint8Array {
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) {
        throw new TypeError("string contains an unpaired high surrogate");
      }
      index++;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw new TypeError("string contains an unpaired low surrogate");
    }
  }
  return textEncoder.encode(value);
}

class Writer {
  #bytes: number[] = [];

  u8(value: number): void {
    this.#bytes.push(value & 0xff);
  }

  u32(value: number): void {
    validateU32(value, "wire u32");
    this.#bytes.push(
      value & 0xff,
      (value >>> 8) & 0xff,
      (value >>> 16) & 0xff,
      (value >>> 24) & 0xff,
    );
  }

  u64(value: bigint): void {
    validateSeed(value);
    for (let shift = 0n; shift < 64n; shift += 8n) this.u8(Number((value >> shift) & 0xffn));
  }

  i64(value: bigint): void {
    if (value < -((1n << 63n) - 1n) || value > (1n << 63n) - 1n) {
      throw new Error("number numerator must be within -(2^63-1)..=2^63-1");
    }
    this.u64(BigInt.asUintN(64, value));
  }

  value(value: MecoValue): void {
    switch (value.kind) {
      case "text":
        this.u8(0);
        this.string(value.value);
        break;
      case "number":
        if (value.denominator < 1n || value.denominator > (1n << 63n) - 1n) {
          throw new Error("number denominator must be within 1..=2^63-1");
        }
        this.u8(1);
        this.i64(value.numerator);
        this.u64(value.denominator);
        break;
      case "boolean":
        this.u8(2);
        this.u8(value.value ? 1 : 0);
        break;
      case "enum":
        this.u8(3);
        this.string(value.value);
        break;
    }
  }

  manifest(manifest: MessageManifest): void {
    this.u32(manifest.messages.length);
    for (const message of manifest.messages) {
      this.string(message.id);
      this.u32(message.arguments.length);
      for (const argument of message.arguments) {
        this.string(argument.name);
        switch (argument.type.kind) {
          case "text":
            this.u8(0);
            break;
          case "number":
            this.u8(1);
            break;
          case "boolean":
            this.u8(2);
            break;
          case "enum":
            this.u8(3);
            this.string(argument.type.name);
            break;
        }
      }
    }
  }

  bytes(value: Uint8Array): void {
    this.u32(value.length);
    for (const byte of value) this.#bytes.push(byte);
  }

  string(value: string): void {
    this.bytes(encodeStrict(value));
  }

  optionalString(value?: string): void {
    if (value === undefined) {
      this.u8(0);
    } else {
      this.u8(1);
      this.string(value);
    }
  }

  finish(): Uint8Array {
    return Uint8Array.from(this.#bytes);
  }
}

class Reader {
  #bytes: Uint8Array;
  #view: DataView;
  #cursor = 0;

  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  u8(): number {
    this.require(1);
    return this.#view.getUint8(this.#cursor++);
  }

  u32(): number {
    this.require(4);
    const value = this.#view.getUint32(this.#cursor, true);
    this.#cursor += 4;
    return value;
  }

  u64(): bigint {
    this.require(8);
    const value = this.#view.getBigUint64(this.#cursor, true);
    this.#cursor += 8;
    return value;
  }

  i64(): bigint {
    return BigInt.asIntN(64, this.u64());
  }

  value(): MecoValue {
    const kind = this.u8();
    if (kind === 0) return { kind: "text", value: this.string() };
    if (kind === 1) {
      return { kind: "number", numerator: this.i64(), denominator: this.u64() };
    }
    if (kind === 2) {
      return { kind: "boolean", value: decodeBoolean(this.u8(), "boolean value") };
    }
    if (kind === 3) return { kind: "enum", value: this.string() };
    throw new Error(`Invalid Mecojoni value kind ${kind}`);
  }

  bytes(): Uint8Array {
    const length = this.u32();
    this.require(length);
    const value = this.#bytes.slice(this.#cursor, this.#cursor + length);
    this.#cursor += length;
    return value;
  }

  string(): string {
    return textDecoder.decode(this.bytes());
  }

  optionalString(): string | undefined {
    const present = this.u8();
    if (present === 0) return undefined;
    if (present !== 1) throw new Error("Invalid optional string flag");
    return this.string();
  }

  diagnostics(): MecoDiagnostic[] {
    const count = this.u32();
    return Array.from({ length: count }, () => {
      const code = this.string();
      const severityByte = this.u8();
      if (severityByte > 1) throw new Error("Invalid diagnostic severity");
      const hasSpan = this.u8();
      let span: SourceSpan | undefined;
      if (hasSpan === 1) {
        span = {
          sourceId: this.u32(),
          start: { byte: this.u64(), scalar: this.u64() },
          end: { byte: this.u64(), scalar: this.u64() },
        };
      } else if (hasSpan !== 0) {
        throw new Error("Invalid diagnostic span flag");
      }
      const message = this.string();
      return { code, severity: severityByte === 0 ? "error" : "warning", span, message };
    });
  }

  finish(): void {
    if (this.#cursor !== this.#bytes.length) throw new Error("Response payload has trailing bytes");
  }

  private require(length: number): void {
    if (length < 0 || this.#cursor + length > this.#bytes.length) {
      throw new Error("Response payload is truncated");
    }
  }
}
