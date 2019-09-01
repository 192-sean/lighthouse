use clap::ArgMatches;
use client::{BeaconChainStartMethod, ClientConfig, Eth2Config};
use eth2_config::{read_from_file, write_to_file};
use lighthouse_bootstrap::Bootstrapper;
use rand::{distributions::Alphanumeric, Rng};
use slog::{crit, info, warn, Logger};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_DATA_DIR: &str = ".lighthouse";
pub const CLIENT_CONFIG_FILENAME: &str = "beacon-node.toml";
pub const ETH2_CONFIG_FILENAME: &str = "eth2-spec.toml";

type Result<T> = std::result::Result<T, String>;
type Config = (ClientConfig, Eth2Config);

/// Gets the fully-initialized global client and eth2 configuration objects.
///
/// The top-level `clap` arguments should be provied as `cli_args`.
///
/// The output of this function depends primarily upon the given `cli_args`, however it's behaviour
/// may be influenced by other external services like the contents of the file system or the
/// response of some remote server.
pub fn get_configs(cli_args: &ArgMatches, log: &Logger) -> Result<Config> {
    let mut builder = ConfigBuilder::new(cli_args, log)?;

    match cli_args.subcommand() {
        ("testnet", Some(sub_cmd_args)) => {
            process_testnet_subcommand(&mut builder, sub_cmd_args, log)?
        }
        // No sub-command assumes a resume operation.
        _ => {
            info!(
                log,
                "Resuming from existing datadir";
                "path" => format!("{:?}", builder.data_dir)
            );

            // If no primary subcommand was given, start the beacon chain from an existing
            // database.
            builder.set_beacon_chain_start_method(BeaconChainStartMethod::Resume);

            // Whilst there is no large testnet or mainnet force the user to specify how they want
            // to start a new chain (e.g., from a genesis YAML file, another node, etc).
            if !builder.data_dir.exists() {
                return Err(
                    "No datadir found. To start a new beacon chain, see `testnet --help`. \
                     Use `--datadir` to specify a different directory"
                        .into(),
                );
            }

            // If the `testnet` command was not provided, attempt to load an existing datadir and
            // continue with an existing chain.
            builder.load_from_datadir()?;
        }
    };

    builder.build(cli_args)
}

/// Process the `testnet` CLI subcommand arguments, updating the `builder`.
fn process_testnet_subcommand(
    builder: &mut ConfigBuilder,
    cli_args: &ArgMatches,
    log: &Logger,
) -> Result<()> {
    if cli_args.is_present("random-datadir") {
        builder.set_random_datadir()?;
    }

    let is_bootstrap = cli_args.subcommand_name() == Some("bootstrap");

    if let Some(path_string) = cli_args.value_of("eth2-config") {
        if is_bootstrap {
            return Err("Cannot supply --eth2-config when using bootsrap".to_string());
        }

        let path = path_string
            .parse::<PathBuf>()
            .map_err(|e| format!("Unable to parse eth2-config path: {:?}", e))?;
        builder.load_eth2_config(path)?;
    } else {
        builder.update_spec_from_subcommand(&cli_args)?;
    }

    if let Some(path_string) = cli_args.value_of("client-config") {
        let path = path_string
            .parse::<PathBuf>()
            .map_err(|e| format!("Unable to parse client config path: {:?}", e))?;
        builder.load_client_config(path)?;
    }

    if cli_args.is_present("force") {
        builder.clean_datadir()?;
    }

    info!(
        log,
        "Creating new datadir";
        "path" => format!("{:?}", builder.data_dir)
    );

    // Start matching on the second subcommand (e.g., `testnet bootstrap ...`).
    match cli_args.subcommand() {
        ("bootstrap", Some(cli_args)) => {
            let server = cli_args
                .value_of("server")
                .ok_or_else(|| "No bootstrap server specified")?;
            let port: Option<u16> = cli_args
                .value_of("libp2p-port")
                .and_then(|s| s.parse::<u16>().ok());

            builder.import_bootstrap_libp2p_address(server, port)?;
            builder.import_bootstrap_eth2_config(server)?;

            builder.set_beacon_chain_start_method(BeaconChainStartMethod::HttpBootstrap {
                server: server.to_string(),
                port,
            })
        }
        ("recent", Some(cli_args)) => {
            let validator_count = cli_args
                .value_of("validator_count")
                .ok_or_else(|| "No validator_count specified")?
                .parse::<usize>()
                .map_err(|e| format!("Unable to parse validator_count: {:?}", e))?;

            let minutes = cli_args
                .value_of("minutes")
                .ok_or_else(|| "No recent genesis minutes supplied")?
                .parse::<u64>()
                .map_err(|e| format!("Unable to parse minutes: {:?}", e))?;

            builder.set_beacon_chain_start_method(BeaconChainStartMethod::RecentGenesis {
                validator_count,
                minutes,
            })
        }
        ("quick", Some(cli_args)) => {
            let validator_count = cli_args
                .value_of("validator_count")
                .ok_or_else(|| "No validator_count specified")?
                .parse::<usize>()
                .map_err(|e| format!("Unable to parse validator_count: {:?}", e))?;

            let genesis_time = cli_args
                .value_of("genesis_time")
                .ok_or_else(|| "No genesis time supplied")?
                .parse::<u64>()
                .map_err(|e| format!("Unable to parse genesis time: {:?}", e))?;

            builder.set_beacon_chain_start_method(BeaconChainStartMethod::Generated {
                validator_count,
                genesis_time,
            })
        }
        _ => return Err("No testnet method specified. See 'testnet --help'.".into()),
    };

    builder.write_configs_to_new_datadir()?;

    Ok(())
}

/// Allows for building a set of configurations based upon `clap` arguments.
struct ConfigBuilder<'a> {
    log: &'a Logger,
    pub data_dir: PathBuf,
    eth2_config: Eth2Config,
    client_config: ClientConfig,
}

impl<'a> ConfigBuilder<'a> {
    /// Create a new builder with default settings.
    pub fn new(cli_args: &'a ArgMatches, log: &'a Logger) -> Result<Self> {
        // Read the `--datadir` flag.
        //
        // If it's not present, try and find the home directory (`~`) and push the default data
        // directory onto it.
        let data_dir: PathBuf = cli_args
            .value_of("datadir")
            .map(|string| PathBuf::from(string))
            .or_else(|| {
                dirs::home_dir().map(|mut home| {
                    home.push(DEFAULT_DATA_DIR);
                    home
                })
            })
            .ok_or_else(|| "Unable to find a home directory for the datadir".to_string())?;

        Ok(Self {
            log,
            data_dir,
            eth2_config: Eth2Config::minimal(),
            client_config: ClientConfig::default(),
        })
    }

    /// Clears any configuration files that would interfere with writing new configs.
    ///
    /// Moves the following files in `data_dir` into a backup directory:
    ///
    /// - Client config
    /// - Eth2 config
    /// - The entire database directory
    pub fn clean_datadir(&mut self) -> Result<()> {
        let backup_dir = {
            let mut s = String::from("backup_");
            s.push_str(&random_string(6));
            self.data_dir.join(s)
        };

        fs::create_dir_all(&backup_dir)
            .map_err(|e| format!("Unable to create config backup dir: {:?}", e))?;

        let move_to_backup_dir = |path: &Path| -> Result<()> {
            let file_name = path
                .file_name()
                .ok_or_else(|| "Invalid path found during datadir clean (no filename).")?;

            let mut new = path.to_path_buf();
            new.pop();
            new.push(backup_dir.clone());
            new.push(file_name);

            let _ = fs::rename(path, new);

            Ok(())
        };

        move_to_backup_dir(&self.data_dir.join(CLIENT_CONFIG_FILENAME))?;
        move_to_backup_dir(&self.data_dir.join(ETH2_CONFIG_FILENAME))?;

        if let Some(db_path) = self.client_config.db_path() {
            move_to_backup_dir(&db_path)?;
        }

        Ok(())
    }

    /// Sets the method for starting the beacon chain.
    pub fn set_beacon_chain_start_method(&mut self, method: BeaconChainStartMethod) {
        self.client_config.beacon_chain_start_method = method;
    }

    /// Import the libp2p address for `server` into the list of bootnodes in `self`.
    ///
    /// If `port` is `Some`, it is used as the port for the `Multiaddr`. If `port` is `None`,
    /// attempts to connect to the `server` via HTTP and retrieve it's libp2p listen port.
    pub fn import_bootstrap_libp2p_address(
        &mut self,
        server: &str,
        port: Option<u16>,
    ) -> Result<()> {
        let bootstrapper = Bootstrapper::from_server_string(server.to_string())?;

        if let Some(server_multiaddr) = bootstrapper.best_effort_multiaddr(port) {
            info!(
                self.log,
                "Estimated bootstrapper libp2p address";
                "multiaddr" => format!("{:?}", server_multiaddr)
            );

            self.client_config
                .network
                .libp2p_nodes
                .push(server_multiaddr);
        } else {
            warn!(
                self.log,
                "Unable to estimate a bootstrapper libp2p address, this node may not find any peers."
            );
        };

        Ok(())
    }

    /// Set the config data_dir to be an random directory.
    ///
    /// Useful for easily spinning up ephemeral testnets.
    pub fn set_random_datadir(&mut self) -> Result<()> {
        let mut s = DEFAULT_DATA_DIR.to_string();
        s.push_str("_random_");
        s.push_str(&random_string(6));

        self.data_dir.pop();
        self.data_dir.push(s);

        Ok(())
    }

    /// Imports an `Eth2Config` from `server`, returning an error if this fails.
    pub fn import_bootstrap_eth2_config(&mut self, server: &str) -> Result<()> {
        let bootstrapper = Bootstrapper::from_server_string(server.to_string())?;

        self.update_eth2_config(bootstrapper.eth2_config()?);

        Ok(())
    }

    fn update_eth2_config(&mut self, eth2_config: Eth2Config) {
        self.eth2_config = eth2_config;
    }

    /// Reads the subcommand and tries to update `self.eth2_config` based up on the `--spec` flag.
    ///
    /// Returns an error if the `--spec` flag is not present in the given `cli_args`.
    pub fn update_spec_from_subcommand(&mut self, cli_args: &ArgMatches) -> Result<()> {
        // Re-initialise the `Eth2Config`.
        //
        // If a CLI parameter is set, overwrite any config file present.
        // If a parameter is not set, use either the config file present or default to minimal.
        let eth2_config = match cli_args.value_of("spec") {
            Some("mainnet") => Eth2Config::mainnet(),
            Some("minimal") => Eth2Config::minimal(),
            Some("interop") => Eth2Config::interop(),
            _ => return Err("Unable to determine specification type.".into()),
        };

        self.client_config.spec_constants = cli_args
            .value_of("spec")
            .expect("Guarded by prior match statement")
            .to_string();
        self.eth2_config = eth2_config;

        Ok(())
    }

    /// Writes the configs in `self` to `self.data_dir`.
    ///
    /// Returns an error if `self.data_dir` already exists.
    pub fn write_configs_to_new_datadir(&mut self) -> Result<()> {
        let db_exists = self
            .client_config
            .db_path()
            .map(|d| d.exists())
            .unwrap_or_else(|| false);

        // Do not permit creating a new config when the datadir exists.
        if db_exists {
            return Err("Database already exists. See `-f` in `testnet --help`".into());
        }

        // Create `datadir` and any non-existing parent directories.
        fs::create_dir_all(&self.data_dir).map_err(|e| {
            crit!(self.log, "Failed to initialize data dir"; "error" => format!("{}", e));
            format!("{}", e)
        })?;

        let client_config_file = self.data_dir.join(CLIENT_CONFIG_FILENAME);
        if client_config_file.exists() {
            return Err(format!(
                "Datadir is not clean, {} exists. See `-f` in `testnet --help`.",
                CLIENT_CONFIG_FILENAME
            ));
        } else {
            // Write the onfig to a TOML file in the datadir.
            write_to_file(
                self.data_dir.join(CLIENT_CONFIG_FILENAME),
                &self.client_config,
            )
            .map_err(|e| format!("Unable to write {} file: {:?}", CLIENT_CONFIG_FILENAME, e))?;
        }

        let eth2_config_file = self.data_dir.join(ETH2_CONFIG_FILENAME);
        if eth2_config_file.exists() {
            return Err(format!(
                "Datadir is not clean, {} exists. See `-f` in `testnet --help`.",
                ETH2_CONFIG_FILENAME
            ));
        } else {
            // Write the config to a TOML file in the datadir.
            write_to_file(self.data_dir.join(ETH2_CONFIG_FILENAME), &self.eth2_config)
                .map_err(|e| format!("Unable to write {} file: {:?}", ETH2_CONFIG_FILENAME, e))?;
        }

        Ok(())
    }

    /// Attempts to load the client and eth2 configs from `self.data_dir`.
    ///
    /// Returns an error if any files are not found or are invalid.
    pub fn load_from_datadir(&mut self) -> Result<()> {
        // Check to ensure the datadir exists.
        //
        // For now we return an error. In the future we may decide to boot a default (e.g.,
        // public testnet or mainnet).
        if !self.data_dir.exists() {
            return Err(
                "No datadir found. Either create a new testnet or specify a different `--datadir`."
                    .into(),
            );
        }

        // If there is a path to a databse in the config, ensure it exists.
        if !self
            .client_config
            .db_path()
            .map(|path| path.exists())
            .unwrap_or_else(|| true)
        {
            return Err(
                "No database found in datadir. Use 'testnet -f' to overwrite the existing \
                 datadir, or specify a different `--datadir`."
                    .into(),
            );
        }

        self.load_eth2_config(self.data_dir.join(ETH2_CONFIG_FILENAME))?;
        self.load_client_config(self.data_dir.join(CLIENT_CONFIG_FILENAME))?;

        Ok(())
    }

    /// Attempts to load the client config from `path`.
    ///
    /// Returns an error if any files are not found or are invalid.
    pub fn load_client_config(&mut self, path: PathBuf) -> Result<()> {
        self.client_config = read_from_file::<ClientConfig>(path.clone())
            .map_err(|e| format!("Unable to parse {:?} file: {:?}", path, e))?
            .ok_or_else(|| format!("{:?} file does not exist", path))?;

        Ok(())
    }

    /// Attempts to load the eth2 config from `path`.
    ///
    /// Returns an error if any files are not found or are invalid.
    pub fn load_eth2_config(&mut self, path: PathBuf) -> Result<()> {
        self.eth2_config = read_from_file::<Eth2Config>(path.clone())
            .map_err(|e| format!("Unable to parse {:?} file: {:?}", path, e))?
            .ok_or_else(|| format!("{:?} file does not exist", path))?;

        Ok(())
    }

    /// Consumes self, returning the configs.
    ///
    /// The supplied `cli_args` should be the base-level `clap` cli_args (i.e., not a subcommand
    /// cli_args).
    pub fn build(mut self, cli_args: &ArgMatches) -> Result<Config> {
        self.eth2_config.apply_cli_args(cli_args)?;
        self.client_config
            .apply_cli_args(cli_args, &mut self.log.clone())?;

        if let Some(bump) = cli_args.value_of("port-bump") {
            let bump = bump
                .parse::<u16>()
                .map_err(|e| format!("Unable to parse port bump: {}", e))?;

            self.client_config.network.libp2p_port += bump;
            self.client_config.network.discovery_port += bump;
            self.client_config.rpc.port += bump;
            self.client_config.rpc.port += bump;
            self.client_config.rest_api.port += bump;
        }

        if self.eth2_config.spec_constants != self.client_config.spec_constants {
            crit!(self.log, "Specification constants do not match.";
                  "client_config" => format!("{}", self.client_config.spec_constants),
                  "eth2_config" => format!("{}", self.eth2_config.spec_constants)
            );
            return Err("Specification constant mismatch".into());
        }

        self.client_config.data_dir = self.data_dir;

        Ok((self.client_config, self.eth2_config))
    }
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .collect::<String>()
}