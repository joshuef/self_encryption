// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Basic example usage of a `SelfEncryptor`.

// For explanation of lint checks, run `rustc -W help` or see
// https://github.com/maidsafe/QA/blob/master/Documentation/Rust%20Lint%20Checks.md
#![forbid(
    arithmetic_overflow,
    mutable_transmutes,
    no_mangle_const_items,
    unknown_crate_types,
    warnings
)]
#![deny(
    bad_style,
    deprecated,
    improper_ctypes,
    missing_docs,
    non_shorthand_field_patterns,
    overflowing_literals,
    stable_features,
    unconditional_recursion,
    unknown_lints,
    unsafe_code,
    unused,
    unused_allocation,
    unused_attributes,
    unused_comparisons,
    unused_features,
    unused_parens,
    while_true
)]
#![warn(
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]
#![allow(
    box_pointers,
    missing_copy_implementations,
    missing_debug_implementations,
    variant_size_differences
)]

use async_trait::async_trait;
use docopt::Docopt;
use self_encryption::{self, test_helpers, DataMap, SelfEncryptor, Storage, StorageError};
use serde::Deserialize;
use std::{
    env,
    error::Error as StdError,
    fmt::{self, Display, Formatter},
    fs::{self, File},
    io::{Error as IoError, Read, Write},
    path::PathBuf,
    string::String,
};
use tiny_keccak::sha3_256;
use unwrap::unwrap;

#[rustfmt::skip]
static USAGE: &str = "
Usage: basic_encryptor -h
       basic_encryptor -e <target>
       basic_encryptor -d <destination>

Options:
    -h, --help      Display this message.
    -e, --encrypt   Encrypt a file.
    -d, --decrypt   Decrypt a file.
";

#[derive(RustcDecodable, Debug, Deserialize)]
struct Args {
    arg_target: Option<String>,
    arg_destination: Option<String>,
    flag_encrypt: bool,
    flag_decrypt: bool,
    flag_help: bool,
}

fn to_hex(ch: u8) -> String {
    fmt::format(format_args!("{:02x}", ch))
}

fn file_name(name: &[u8]) -> String {
    let mut string = String::new();
    for ch in name {
        string.push_str(&to_hex(*ch));
    }
    string
}

#[derive(Debug)]
struct DiskBasedStorageError {
    io_error: IoError,
}

impl Display for DiskBasedStorageError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "I/O error getting/putting: {}", self.io_error)
    }
}

impl StdError for DiskBasedStorageError {
    fn description(&self) -> &str {
        "DiskBasedStorage Error"
    }
}

impl From<IoError> for DiskBasedStorageError {
    fn from(error: IoError) -> DiskBasedStorageError {
        DiskBasedStorageError { io_error: error }
    }
}

impl StorageError for DiskBasedStorageError {}

struct DiskBasedStorage {
    pub storage_path: String,
}

impl DiskBasedStorage {
    fn calculate_path(&self, name: &[u8]) -> PathBuf {
        let mut path = PathBuf::from(self.storage_path.clone());
        path.push(file_name(name));
        path
    }
}

#[async_trait]
impl Storage for DiskBasedStorage {
    type Error = DiskBasedStorageError;

    async fn get(&mut self, name: &[u8]) -> Result<Vec<u8>, DiskBasedStorageError> {
        let path = self.calculate_path(name);
        let mut file = File::open(&path)?;
        let mut data = Vec::new();
        let _ = file.read_to_end(&mut data);

        Ok(data)
    }

    async fn put(&mut self, name: Vec<u8>, data: Vec<u8>) -> Result<(), DiskBasedStorageError> {
        let path = self.calculate_path(&name);
        let mut file = File::create(&path)?;

        file.write_all(&data[..])
            .map(|_| {
                println!("Chunk written to {:?}", path);
            })
            .map_err(From::from)
    }

    async fn generate_address(&self, data: &[u8]) -> Vec<u8> {
        sha3_256(data).to_vec()
    }
}

// use tokio to enable an async main func
#[tokio::main]
async fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());
    if args.flag_help {
        println!("{:?}", args)
    }

    let mut chunk_store_dir = env::temp_dir();
    chunk_store_dir.push("chunk_store_test/");
    let _ = fs::create_dir(chunk_store_dir.clone());
    let mut storage = DiskBasedStorage {
        storage_path: unwrap!(chunk_store_dir.to_str()).to_owned(),
    };

    let mut data_map_file = chunk_store_dir;
    data_map_file.push("data_map");

    if args.flag_encrypt && args.arg_target.is_some() {
        if let Ok(mut file) = File::open(unwrap!(args.arg_target.clone())) {
            match file.metadata() {
                Ok(metadata) => {
                    if metadata.len() > self_encryption::MAX_FILE_SIZE as u64 {
                        return println!(
                            "File size too large {} is greater than 1GB",
                            metadata.len()
                        );
                    }
                }
                Err(error) => return println!("{}", error.to_string()),
            }

            let mut data = Vec::new();
            match file.read_to_end(&mut data) {
                Ok(_) => (),
                Err(error) => return println!("{}", error.to_string()),
            }

            let se = SelfEncryptor::new(storage, DataMap::None)
                .expect("Encryptor construction shouldn't fail.");
            se.write(&data, 0)
                .await
                .expect("Writing to encryptor shouldn't fail.");
            let (data_map, old_storage) =
                se.close().await.expect("Closing encryptor shouldn't fail.");
            storage = old_storage;

            match File::create(data_map_file.clone()) {
                Ok(mut file) => {
                    let encoded = test_helpers::serialise(&data_map);
                    match file.write_all(&encoded[..]) {
                        Ok(_) => println!("Data map written to {:?}", data_map_file),
                        Err(error) => {
                            println!(
                                "Failed to write data map to {:?} - {:?}",
                                data_map_file, error
                            );
                        }
                    }
                }
                Err(error) => {
                    println!(
                        "Failed to create data map at {:?} - {:?}",
                        data_map_file, error
                    );
                }
            }
        } else {
            println!("Failed to open {}", unwrap!(args.arg_target.clone()));
        }
    }

    if args.flag_decrypt && args.arg_destination.is_some() {
        if let Ok(mut file) = File::open(data_map_file.clone()) {
            let mut data = Vec::new();
            let _ = unwrap!(file.read_to_end(&mut data));

            if let Some(data_map) = test_helpers::deserialise::<DataMap>(&data) {
                let se = SelfEncryptor::new(storage, data_map)
                    .expect("Encryptor construction shouldn't fail.");
                let length = se.len().await;
                if let Ok(mut file) = File::create(unwrap!(args.arg_destination.clone())) {
                    let content = se
                        .read(0, length)
                        .await
                        .expect("Reading from encryptor shouldn't fail.");
                    match file.write_all(&content[..]) {
                        Err(error) => println!("File write failed - {:?}", error),
                        Ok(_) => println!(
                            "File decrypted to {:?}",
                            unwrap!(args.arg_destination.clone())
                        ),
                    };
                } else {
                    println!("Failed to create {}", unwrap!(args.arg_destination.clone()));
                }
            } else {
                println!("Failed to parse data map - possible corruption");
            }
        } else {
            println!("Failed to open data map at {:?}", data_map_file);
        }
    }
}
