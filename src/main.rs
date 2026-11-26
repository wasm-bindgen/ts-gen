mod generator;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgGroup, Parser};
use ts_gen::input;
use ts_gen_abi::ir::Ir;
use ts_gen_abi::profile::Profile;

/// Generate language bindings from TypeScript .d.ts files.
///
/// The input is `.d.ts` files or the IR they lower to (`.tw`). By default
/// the bindings come from a language generator (`--lang`); `--ir`, `--json`
/// and `--bridge` emit the IR, its lowered JSON, or the JS bridge of a
/// compiled language module instead. Arguments after `--` go to the
/// language generator.
#[derive(Parser, Debug)]
#[command(name = "ts-gen", version, about)]
#[command(group(ArgGroup::new("emit").multiple(false)))]
struct Cli {
    /// `.d.ts` files or directories, or one `.tw` IR file.
    #[arg(short, long, required = true, num_args = 1..)]
    input: Vec<PathBuf>,

    /// Where the output goes: the language generator's output, the file
    /// `--ir` or `--json` writes (stdout when omitted or `-`), or the
    /// directory `--bridge` writes into.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// The language to generate bindings for, `wasm-bindgen` by default,
    /// by running `ts-gen-<LANG>`.
    ///
    /// The generator is the binary `TS_GEN_<LANG>` names
    /// (`TS_GEN_WASM_BINDGEN`), or else one installed on first use, at
    /// this version, into ts-gen's cache directory. It is passed the
    /// input, the output, the `.d.ts` options given here, and the
    /// arguments after `--`.
    #[arg(long, value_name = "LANG", group = "emit")]
    lang: Option<String>,

    /// Emit the ts-gen IR (`IR.md`). A `.tw` input is validated and
    /// printed again.
    #[arg(long, group = "emit")]
    ir: bool,

    /// Emit the IR lowered to JSON under `--profile`, for a language
    /// generator to read.
    #[arg(long, group = "emit")]
    json: bool,

    /// Emit the bridge of this compiled language module (`.wasm` or
    /// `.wat`) under `--profile`: `bridge.js` and `bridge.wasm`, written
    /// into the `--output` directory.
    #[arg(long, value_name = "MODULE", group = "emit")]
    bridge: Option<PathBuf>,

    /// The profile of `--json` and `--bridge`: a file holding the profile
    /// text, or the text itself (`all-host, results = single`). The
    /// default is `all-linear`.
    #[arg(long, value_name = "PROFILE")]
    profile: Option<String>,

    /// Also write `bridge.wat` (with `--bridge`).
    #[arg(long)]
    wat: bool,

    /// Shape `bridge.js` as a factory of bridges, one per instance of the
    /// language module (with `--bridge`).
    #[arg(long)]
    factory: bool,

    /// The lib IRs (`lib name;`) the IR's `with` names. With `.d.ts`
    /// input, the input's lib types resolve against them instead of
    /// being stubbed.
    #[arg(long = "with", num_args = 1..)]
    with: Vec<PathBuf>,

    /// JS module specifier (required when .d.ts has top-level exports).
    #[arg(long)]
    lib_name: Option<String>,

    /// Side `.d.ts` files whose `@tw-abi` and `@throws` JSDoc tags are
    /// layered onto the input declarations, matched by declaration and
    /// member name.
    ///
    /// Upstream declaration files carry no such tags and editing a
    /// vendored copy does not survive an upgrade. A side file re-declares
    /// only the members it annotates (`interface Response { /** @tw-abi u16
    /// */ readonly status: number; }`); the tags are applied to the input's
    /// declarations and the side file itself is not emitted. A name that
    /// matches nothing is an error.
    #[arg(long = "annotate", num_args = 1..)]
    annotate: Vec<PathBuf>,

    /// Keep `T | null | undefined` three-state in the IR, as `T??`.
    ///
    /// By default it's `T?`, which writes `undefined`, a value TS accepts
    /// there. `T | null` is `@js(null) T?` either way, since TS rejects
    /// `undefined` for it. Every target reads the IR this produces.
    #[arg(long = "strict-nullable")]
    strict_nullable: bool,

    /// What the module provides to JS, lowered as IR exports: a `.d.ts`
    /// whose exports are the module's, or `Name[.member,...]`, an input
    /// interface the default export implements.
    #[arg(long)]
    provides: Option<String>,

    /// A callback parameter called at most once, `Owner.member.param` or
    /// `function.param` in JS names: `@once` in the IR.
    #[arg(long = "once", num_args = 1..)]
    once: Vec<String>,

    /// Don't make future-returning calls cancellable through an
    /// `AbortSignal` dictionary field.
    #[arg(long = "no-cancel")]
    no_cancel: bool,

    /// A concrete bound, `Decl.Param=T,U` in IR types (`Map.K=Str,f64`).
    #[arg(long = "bound", num_args = 1..)]
    bound: Vec<String>,

    /// Produce the lib IR `name` of the `--root` declarations.
    #[arg(long = "emit-lib", value_name = "NAME", requires = "root")]
    emit_lib: Option<String>,

    /// A root declaration of `--emit-lib`, by global name.
    #[arg(long = "root", num_args = 1.., value_delimiter = ',')]
    root: Vec<String>,

    /// Arguments for the language generator.
    #[arg(last = true, value_name = "GENERATOR ARGS")]
    generator_args: Vec<OsString>,
}

impl Cli {
    /// The options that shape how `.d.ts` input is read and lowered, as
    /// given, for the language generator's command line.
    fn dts_args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        let mut list = |flag: &str, values: Vec<OsString>| {
            if !values.is_empty() {
                args.push(flag.into());
                args.extend(values);
            }
        };
        let strs = |v: &[String]| v.iter().map(OsString::from).collect::<Vec<_>>();
        let paths = |v: &[PathBuf]| v.iter().map(OsString::from).collect::<Vec<_>>();
        list("--lib-name", strs(self.lib_name.as_slice()));
        list("--annotate", paths(&self.annotate));
        list("--provides", strs(self.provides.as_slice()));
        list("--once", strs(&self.once));
        list("--bound", strs(&self.bound));
        list("--with", paths(&self.with));
        list("--emit-lib", strs(self.emit_lib.as_slice()));
        list("--root", strs(&self.root));
        if self.strict_nullable {
            args.push("--strict-nullable".into());
        }
        if self.no_cancel {
            args.push("--no-cancel".into());
        }
        args
    }

    /// The first option given that only `.d.ts` input takes.
    fn dts_only(&self) -> Option<&'static str> {
        [
            ("--lib-name", self.lib_name.is_some()),
            ("--annotate", !self.annotate.is_empty()),
            ("--strict-nullable", self.strict_nullable),
            ("--provides", self.provides.is_some()),
            ("--once", !self.once.is_empty()),
            ("--no-cancel", self.no_cancel),
            ("--bound", !self.bound.is_empty()),
            ("--emit-lib", self.emit_lib.is_some()),
            ("--root", !self.root.is_empty()),
        ]
        .into_iter()
        .find_map(|(flag, given)| given.then_some(flag))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.bridge.is_none() {
        if let Some(flag) = [("--wat", cli.wat), ("--factory", cli.factory)]
            .into_iter()
            .find_map(|(flag, given)| given.then_some(flag))
        {
            bail!("{flag} applies to --bridge");
        }
        if cli.profile.is_some() && !cli.json {
            bail!("--profile applies to --json and --bridge");
        }
    }
    if !(cli.ir || cli.json || cli.bridge.is_some()) {
        std::process::exit(generate(&cli)?);
    }
    if !cli.generator_args.is_empty() {
        bail!("the arguments after `--` are a language generator's, and none runs here");
    }
    let libs = read_libs(&cli.with)?;
    let ir = read_ir(&cli, &libs)?;
    if cli.ir {
        return write_text(cli.output.as_deref(), &ts_gen_abi::text::print(&ir));
    }
    let profile = match &cli.profile {
        Some(v) => {
            let src = std::fs::read_to_string(v).unwrap_or_else(|_| v.clone());
            Profile::parse(&src).map_err(|e| anyhow!("--profile {v}: {e}"))?
        }
        None => Profile::default(),
    };
    let Some(module) = &cli.bridge else {
        let named = ts_gen_abi::resolve::libs_for(&ir, &libs).unwrap_or_default();
        let json = ts_gen_abi::json::lower_with(&ir, &named, &profile).pretty();
        return write_text(cli.output.as_deref(), &json);
    };
    let Some(dir) = &cli.output else {
        bail!("--bridge writes into a directory: pass --output <dir>");
    };
    let language = match module.extension().is_some_and(|e| e == "wat") {
        true => wat::parse_file(module).map_err(|e| anyhow!("{e}"))?,
        false => {
            std::fs::read(module).with_context(|| format!("Failed to read {}", module.display()))?
        }
    };
    let opts = ts_gen_bridge::Options {
        factory: cli.factory,
    };
    let bridge = ts_gen_bridge::generate_opts(&ir, &libs, &profile, &language, opts)
        .map_err(|e| anyhow!("{e}"))?;
    for w in &bridge.warnings {
        eprintln!("[ts-gen] bridge: {w}");
    }
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).with_context(|| format!("Failed to write {}", path.display()))
    };
    write("bridge.js", bridge.js.as_bytes())?;
    write("bridge.wasm", &bridge.wasm)?;
    if cli.wat {
        write("bridge.wat", bridge.wat.as_bytes())?;
    }
    eprintln!("[ts-gen] Wrote {}", dir.display());
    Ok(())
}

/// Runs the language generator and returns its exit code.
fn generate(cli: &Cli) -> Result<i32> {
    let lang = cli.lang.as_deref().unwrap_or("wasm-bindgen");
    generator::check_lang(lang)?;
    let mut args: Vec<OsString> = vec!["--input".into()];
    args.extend(cli.input.iter().map(OsString::from));
    if let Some(output) = &cli.output {
        args.push("--output".into());
        args.push(output.into());
    }
    args.extend(cli.dts_args());
    args.extend(cli.generator_args.iter().cloned());
    generator::run(lang, &args)
}

fn read_libs(paths: &[PathBuf]) -> Result<Vec<Ir>> {
    let mut libs = Vec::new();
    for path in paths {
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read --with {}", path.display()))?;
        let lib =
            ts_gen_abi::text::parse(&src).map_err(|e| anyhow!("--with {}: {e}", path.display()))?;
        if lib.lib.is_none() {
            bail!("--with {}: not a lib IR (no `lib name;`)", path.display());
        }
        libs.push(lib);
    }
    Ok(libs)
}

fn is_tw(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "tw") && !path.is_dir()
}

/// The input's IR, validated: a `.tw` file's, or the `.d.ts` files'
/// lowering.
fn read_ir(cli: &Cli, libs: &[Ir]) -> Result<Ir> {
    let ir = match &cli.input[..] {
        [path] if is_tw(path) => {
            if let Some(flag) = cli.dts_only() {
                bail!("{flag} applies to .d.ts input, and the input is an IR");
            }
            let src = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            ts_gen_abi::text::parse(&src).map_err(|e| anyhow!("{}: {e}", path.display()))?
        }
        inputs if inputs.iter().any(|p| is_tw(p)) => {
            bail!("a .tw input is the only input: it can't be combined with others")
        }
        inputs => lower(cli, inputs, libs)?,
    };
    let checks = ts_gen_abi::validate::Options {
        libs,
        ..Default::default()
    };
    if let Err(errors) = ts_gen_abi::validate::validate_with(&ir, checks) {
        for e in &errors {
            eprintln!("[ts-gen] ir: {e}");
        }
        bail!("the IR doesn't validate");
    }
    Ok(ir)
}

fn lower(cli: &Cli, inputs: &[PathBuf], libs: &[Ir]) -> Result<Ir> {
    let files = input::resolve_input_files(inputs)?;
    if files.is_empty() {
        bail!("No .d.ts files found in the specified input paths");
    }
    eprintln!("[ts-gen] Processing {} file(s)...", files.len());
    let load = input::Load {
        lib_name: cli.lib_name.as_deref(),
        annotate: &cli.annotate,
    };
    let (module, mut gctx) = input::load(&files, &load, |_| {})?;
    let mut opts = ts_gen::lower::Options {
        once: cli.once.clone(),
        cancel: !cli.no_cancel,
        strict_nullable: cli.strict_nullable,
        with: libs.to_vec(),
        ..Default::default()
    };
    for b in &cli.bound {
        opts.bounds
            .push(ts_gen::lower::parse_bound(b).map_err(|e| anyhow!("--bound {e}"))?);
    }
    if let Some(spec) = &cli.provides {
        opts.provides = ts_gen::provides::resolve(spec, &module, &mut gctx)?;
    }
    if let Some(name) = &cli.emit_lib {
        opts.lib = Some(ts_gen::lower::LibOptions {
            name: name.clone(),
            roots: cli.root.clone(),
        });
    }
    let lowered = ts_gen::lower::lower_with(&module, &gctx, &opts);
    for d in &lowered.diagnostics {
        eprintln!("[ts-gen] ir: {d}");
    }
    Ok(lowered.ir)
}

/// Writes `text` to `path`, or to stdout for no path or `-`.
fn write_text(path: Option<&Path>, text: &str) -> Result<()> {
    match path.filter(|p| p.as_os_str() != "-") {
        None => print!("{text}"),
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }
            std::fs::write(path, text)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            eprintln!("[ts-gen] Wrote {}", path.display());
        }
    }
    Ok(())
}
