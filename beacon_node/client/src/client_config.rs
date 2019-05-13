use clap::ArgMatches;
use db::DBType;
use eth2_libp2p::multiaddr::Protocol;
// use eth2_libp2p::multiaddr::ToMultiaddr;
use eth2_libp2p::Multiaddr;
use fork_choice::ForkChoiceAlgorithm;
use network::{ChainType, NetworkConfig};
use slog::{error, o, Drain, Level};
use std::fs;
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use types::ChainSpec;

/// Stores the client configuration for this Lighthouse instance.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub data_dir: PathBuf,
    pub spec: ChainSpec,
    pub net_conf: network::NetworkConfig,
    pub fork_choice: ForkChoiceAlgorithm,
    pub db_type: DBType,
    pub db_name: PathBuf,
    pub rpc_conf: rpc::RPCConfig,
    //pub ipc_conf:
}

impl Default for ClientConfig {
    /// Build a new lighthouse configuration from defaults.
    fn default() -> Self {
        let data_dir = {
            let home = dirs::home_dir().expect("Unable to determine home dir.");
            home.join(".lighthouse/")
        };
        fs::create_dir_all(&data_dir)
            .unwrap_or_else(|_| panic!("Unable to create {:?}", &data_dir));

        // currently lighthouse spec
        let default_spec = ChainSpec::lighthouse_testnet();
        let chain_type = ChainType::from(default_spec.chain_id);
        // builds a chain-specific network config
        let net_conf = NetworkConfig::from(chain_type);

        Self {
            data_dir: data_dir.clone(),
            // default to foundation for chain specs
            spec: default_spec,
            net_conf,
            // default to bitwise LMD Ghost
            fork_choice: ForkChoiceAlgorithm::BitwiseLMDGhost,
            // default to memory db for now
            db_type: DBType::Memory,
            // default db name for disk-based dbs
            db_name: data_dir.join("chain.db"),
            rpc_conf: rpc::RPCConfig::default(),
        }
    }
}

impl ClientConfig {
    /// Parses the CLI arguments into a `Config` struct.
    pub fn parse_args(args: ArgMatches) -> Result<(slog::Logger, Self), &'static str> {
        let mut config = ClientConfig::default();

        /* Logging related arguments */

        // build the initial logger
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::CompactFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build();

        let drain = match args.occurrences_of("verbosity") {
            0 => drain.filter_level(Level::Info),
            1 => drain.filter_level(Level::Debug),
            2 => drain.filter_level(Level::Trace),
            _ => drain.filter_level(Level::Trace),
        };

        let log = slog::Logger::root(drain.fuse(), o!());

        /* Network related arguments */

        // Custom p2p listen port
        if let Some(port_str) = args.value_of("port") {
            if let Ok(port) = port_str.parse::<u16>() {
                config.net_conf.listen_port = port;
                // update the listening multiaddrs
                for address in &mut config.net_conf.listen_addresses {
                    address.pop();
                    address.push(Protocol::Tcp(port));
                }
            } else {
                error!(log, "Invalid port"; "port" => port_str);
                return Err("Invalid port");
            }
        }
        // Custom listening address ipv4/ipv6
        // TODO: Handle list of addresses
        if let Some(listen_address_str) = args.value_of("listen-address") {
            if let Ok(listen_address) = listen_address_str.parse::<IpAddr>() {
                let mut multiaddr = Multiaddr::from(listen_address);
                multiaddr.push(Protocol::Tcp(config.net_conf.listen_port));
                config.net_conf.listen_addresses = vec![multiaddr];
            } else {
                error!(log, "Invalid IP Address"; "Address" => listen_address_str);
                return Err("Invalid IP Address");
            }
        }

        // Custom bootnodes
        // TODO: Handle list of addresses
        if let Some(boot_addresses_str) = args.value_of("boot-nodes") {
            if let Ok(boot_address) = boot_addresses_str.parse::<Multiaddr>() {
                config.net_conf.boot_nodes.append(&mut vec![boot_address]);
            } else {
                error!(log, "Invalid Bootnode multiaddress"; "Multiaddr" => boot_addresses_str);
                return Err("Invalid IP Address");
            }
        }

        /* Filesystem related arguments */

        // Custom datadir
        if let Some(dir) = args.value_of("datadir") {
            config.data_dir = PathBuf::from(dir.to_string());
        };

        /* RPC related arguments */

        if args.is_present("rpc") {
            config.rpc_conf.enabled = true;
        }

        if let Some(rpc_address) = args.value_of("rpc-address") {
            if let Ok(listen_address) = rpc_address.parse::<Ipv4Addr>() {
                config.rpc_conf.listen_address = listen_address;
            } else {
                error!(log, "Invalid RPC listen address"; "Address" => rpc_address);
                return Err("Invalid RPC listen address");
            }
        }

        if let Some(rpc_port) = args.value_of("rpc-port") {
            if let Ok(port) = rpc_port.parse::<u16>() {
                config.rpc_conf.port = port;
            } else {
                error!(log, "Invalid RPC port"; "port" => rpc_port);
                return Err("Invalid RPC port");
            }
        }

        Ok((log, config))
    }
}
