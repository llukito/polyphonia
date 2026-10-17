//! `poly`: command-line front end to poly-engine, for testing and demos.
//!
//! Keys are stored in plain text files (hex) so they are easy to inspect.
//! This is for testing only; the Mac app keeps secret keys in the Keychain.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use poly_engine::{
    Algorithm, CarrierKind, CarrierOptions, DecryptRequest, EncryptRequest, Engine, PolyError,
    contact_id,
};

/// First line of a public key file, followed by the algorithm name.
const PUBLIC_KEY_HEADER: &str = "polyphonia-public-key v1";
/// First line of a secret key file, followed by the algorithm name.
const SECRET_KEY_HEADER: &str = "polyphonia-secret-key v1";
/// Owner read/write only, for secret key files on Unix.
#[cfg(unix)]
const SECRET_KEY_MODE: u32 = 0o600;

#[derive(Parser)]
#[command(
    name = "poly",
    version,
    about = "Polyphonia: hide encrypted messages in images and music"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a key pair: writes <OUT>.pub and <OUT>.key
    Keygen {
        #[arg(long, value_enum)]
        algo: Algo,
        /// Base name for the key files
        #[arg(long)]
        out: PathBuf,
    },
    /// Encrypt a message into a carrier file
    Encrypt {
        /// Recipient's public key file (<name>.pub)
        #[arg(long)]
        to: PathBuf,
        /// Algorithm (defaults to the key file's algorithm)
        #[arg(long, value_enum)]
        algo: Option<Algo>,
        #[arg(long, value_enum, default_value = "raw")]
        carrier: CarrierArg,
        /// The message
        #[arg(long)]
        text: String,
        /// Hidden mode: harmless decoy text shown without the passphrase
        #[arg(long, requires = "passphrase")]
        decoy: Option<String>,
        /// Hidden mode: passphrase agreed in person
        #[arg(long, requires = "decoy")]
        passphrase: Option<String>,
        /// Cover image for image carriers
        #[arg(long)]
        cover: Option<PathBuf>,
        /// Output file (the carrier's extension is added if missing)
        #[arg(long)]
        out: PathBuf,
    },
    /// Decrypt a carrier file (carrier is auto-detected)
    Decrypt {
        /// Your secret key file (<name>.key)
        #[arg(long)]
        key: PathBuf,
        /// Your public key file (<name>.pub)
        #[arg(long = "pub")]
        public: PathBuf,
        #[arg(long)]
        passphrase: Option<String>,
        file: PathBuf,
    },
    /// Show the detected carrier and envelope header (no key needed)
    Inspect { file: PathBuf },
    /// Simulate messenger damage (JPEG recompression, resizing, Opus) [role 2]
    Damage {
        file: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Algo {
    Fake,
    Mlkem,
    Rsa,
}

impl From<Algo> for Algorithm {
    fn from(a: Algo) -> Self {
        match a {
            Algo::Fake => Algorithm::Fake,
            Algo::Mlkem => Algorithm::MlKem768,
            Algo::Rsa => Algorithm::Rsa2048,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CarrierArg {
    Raw,
    StealthPng,
    RobustPng,
    Wav,
    Midi,
}

impl From<CarrierArg> for CarrierKind {
    fn from(c: CarrierArg) -> Self {
        match c {
            CarrierArg::Raw => CarrierKind::RawDebug,
            CarrierArg::StealthPng => CarrierKind::StealthPng,
            CarrierArg::RobustPng => CarrierKind::RobustPng,
            CarrierArg::Wav => CarrierKind::MusicWav,
            CarrierArg::Midi => CarrierKind::MusicMidi,
        }
    }
}

fn algorithm_from_name(name: &str) -> Result<Algorithm> {
    Algorithm::ALL
        .into_iter()
        .find(|a| a.name() == name)
        .ok_or_else(|| anyhow!("unknown algorithm {name:?} in key file"))
}

fn write_key(path: &Path, header: &str, algorithm: Algorithm, key: &[u8]) -> Result<()> {
    let contents = format!("{header} {}\n{}\n", algorithm.name(), hex::encode(key));
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

fn read_key(path: &Path, header: &str) -> Result<(Algorithm, Vec<u8>)> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = contents.lines();
    let first = lines.next().unwrap_or_default();
    let algo = first
        .strip_prefix(header)
        .map(str::trim)
        .ok_or_else(|| anyhow!("{} is not a {header} file", path.display()))?;
    let key = hex::decode(lines.next().unwrap_or_default().trim())
        .with_context(|| format!("{}: key is not valid hex", path.display()))?;
    Ok((algorithm_from_name(algo)?, key))
}

/// Turn engine errors into messages a person can act on.
fn explain(e: PolyError) -> anyhow::Error {
    match e {
        PolyError::ChecksumMismatch => anyhow!(
            "{e}\nhint: the messenger probably recompressed it; ask them to resend it as a document/file"
        ),
        PolyError::UnrecognizedFile => {
            anyhow!("{e}\nhint: this file does not contain a Polyphonia message")
        }
        other => anyhow!(other),
    }
}

fn main() -> Result<()> {
    let engine = Engine::default();
    match Cli::parse().command {
        Command::Keygen { algo, out } => {
            let algorithm = Algorithm::from(algo);
            let id = engine.generate_identity(algorithm).map_err(explain)?;
            let public = out.with_extension("pub");
            let secret = out.with_extension("key");
            write_key(&public, PUBLIC_KEY_HEADER, algorithm, &id.public_key)?;
            write_key(&secret, SECRET_KEY_HEADER, algorithm, &id.secret_key)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&secret, fs::Permissions::from_mode(SECRET_KEY_MODE))?;
            }
            println!("wrote {} and {}", public.display(), secret.display());
            println!("contact id: {}", hex::encode(contact_id(&id.public_key)));
        }
        Command::Encrypt {
            to,
            algo,
            carrier,
            text,
            decoy,
            passphrase,
            cover,
            mut out,
        } => {
            let (key_algorithm, recipient_public_key) = read_key(&to, PUBLIC_KEY_HEADER)?;
            let algorithm = algo.map_or(key_algorithm, Algorithm::from);
            if algorithm != key_algorithm {
                bail!(
                    "{} is a {} key but --algo is {}",
                    to.display(),
                    key_algorithm.name(),
                    algorithm.name()
                );
            }
            let cover_image = cover
                .map(|p| fs::read(&p).with_context(|| format!("reading {}", p.display())))
                .transpose()?;
            let file = engine
                .encrypt(&EncryptRequest {
                    message: text,
                    decoy,
                    passphrase,
                    recipient_public_key,
                    algorithm,
                    carrier: carrier.into(),
                    options: CarrierOptions {
                        cover_image,
                        ..Default::default()
                    },
                })
                .map_err(explain)?;
            if out.extension().is_none() {
                out.set_extension(file.extension);
            }
            fs::write(&out, &file.bytes).with_context(|| format!("writing {}", out.display()))?;
            println!(
                "wrote {} ({} bytes, carrier {}, algorithm {})",
                out.display(),
                file.bytes.len(),
                file.carrier.name(),
                file.algorithm.name()
            );
        }
        Command::Decrypt {
            key,
            public,
            passphrase,
            file,
        } => {
            let (_, my_secret_key) = read_key(&key, SECRET_KEY_HEADER)?;
            let (_, my_public_key) = read_key(&public, PUBLIC_KEY_HEADER)?;
            let file = fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let d = engine
                .decrypt(&DecryptRequest {
                    file,
                    my_secret_key,
                    my_public_key,
                    passphrase,
                })
                .map_err(explain)?;
            println!("text:      {}", d.text);
            println!("slot:      {:?}", d.slot);
            println!("carrier:   {}", d.carrier.name());
            println!("algorithm: {}", d.algorithm.name());
        }
        Command::Inspect { file } => {
            let bytes = fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let i = engine.inspect(&bytes).map_err(explain)?;
            let env = &i.envelope;
            println!("carrier:     {}", i.carrier.name());
            println!("version:     {}", env.version);
            println!(
                "algorithm:   {} ({:#04x})",
                env.algorithm.name(),
                env.algorithm.id()
            );
            println!(
                "flags:       {:#010b}{}",
                env.flags,
                if env.is_session() { " (SESSION)" } else { "" }
            );
            println!("contact id:  {}", hex::encode(env.contact_id));
            println!("payload len: {}", env.payload.len());
            println!("crc:         ok");
        }
        Command::Damage { .. } => {
            eprintln!("poly damage: not implemented yet (role 2)");
            std::process::exit(1);
        }
    }
    Ok(())
}
