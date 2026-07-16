const WIRE_VERSION = 1;
const OP_PACKAGE_CREATE = 1;
const OP_COMPILE = 2;
const OP_GENERATE_TYPED = 4;

const PAYLOAD_ERROR = 0;
const PAYLOAD_PACKAGE = 1;
const PAYLOAD_COMPILE = 2;
const PAYLOAD_GENERATE = 3;

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

export interface GenerationOutput {
  text: string;
  entry: string;
  expansions: number;
  samplerWords: number;
  bindings: BindingOutput[];
  selections: SelectionOutput[];
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
  baseWeight: { numerator: bigint; denominator: bigint };
  normalizedWeight: bigint;
}

export interface SelectionOutput {
  rule: string;
  selectedProduction: number;
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

  compile(mecoPackage: MecoPackage): MecoResult<CompiledGrammar> {
    let decoded: DecodedResult | undefined;
    try {
      this.assertOwner(mecoPackage);
      const writer = request();
      writer.u32(mecoPackage.handle);
      decoded = this.invoke(OP_COMPILE, writer.finish());
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

  compilePackage(description: PackageDescription): MecoResult<CompiledGrammar> {
    const created = this.createPackage(description);
    if (!created.ok) return created;
    try {
      return this.compile(created.value);
    } finally {
      created.value.dispose();
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
      const data = Object.entries(options.data ?? {}).sort(([left], [right]) =>
        left < right ? -1 : left > right ? 1 : 0
      );
      writer.u32(data.length);
      for (const [name, value] of data) {
        writer.string(name);
        writer.value(value);
      }
      const decoded = this.invoke(OP_GENERATE_TYPED, writer.finish());
      if (decoded.status !== 0) return decodeFailure(decoded);
      expectKind(decoded, PAYLOAD_GENERATE);
      const value = {
        text: decoded.reader.string(),
        entry: decoded.reader.string(),
        expansions: decoded.reader.u32(),
        samplerWords: decoded.reader.u32(),
        bindings: Array.from({ length: decoded.reader.u32() }, () => ({
          name: decoded.reader.string(),
          emitted: decodeBoolean(decoded.reader.u8(), "binding emitted flag"),
          value: decoded.reader.value(),
        })),
        selections: Array.from({ length: decoded.reader.u32() }, () => ({
          rule: decoded.reader.string(),
          selectedProduction: decoded.reader.u32(),
          eligible: Array.from({ length: decoded.reader.u32() }, () => ({
            production: decoded.reader.u32(),
            baseWeight: {
              numerator: decoded.reader.i64(),
              denominator: decoded.reader.u64(),
            },
            normalizedWeight: decoded.reader.u64(),
          })),
        })),
      };
      decoded.reader.finish();
      return { ok: true, value, diagnostics: [] };
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
