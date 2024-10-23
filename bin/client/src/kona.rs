#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations, missing_docs, unreachable_pub, rustdoc::all)]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![no_std]
#![cfg_attr(any(target_arch = "mips", target_arch = "riscv64"), no_main)]

extern crate alloc;

use alloc::sync::Arc;
use alloy_primitives::hex;
use celestia_types::nmt::Namespace;
use dotenvy::var;
use kona_client::{
    celestia::celestia_provider::OracleCelestiaProvider,
    l1::{DerivationDriver, OracleBlobProvider, OracleL1ChainProvider},
    l2::OracleL2ChainProvider,
    BootInfo, CachingOracle,
};
use kona_common_proc::client_entry;

pub(crate) mod fault;
use fault::{fpvm_handle_register, HINT_WRITER, ORACLE_READER};

/// The size of the LRU cache in the oracle.
const ORACLE_LRU_SIZE: usize = 1024;

#[client_entry(100_000_000)]
fn main() -> Result<()> {
    #[cfg(feature = "tracing-subscriber")]
    {
        use anyhow::anyhow;
        use tracing::Level;

        let subscriber = tracing_subscriber::fmt().with_max_level(Level::DEBUG).finish();
        tracing::subscriber::set_global_default(subscriber).map_err(|e| anyhow!(e))?;
    }

    kona_common::block_on(async move {
        ////////////////////////////////////////////////////////////////
        //                          PROLOGUE                          //
        ////////////////////////////////////////////////////////////////

        let oracle = Arc::new(CachingOracle::new(ORACLE_LRU_SIZE, ORACLE_READER, HINT_WRITER));
        let boot = Arc::new(BootInfo::load(oracle.as_ref()).await?);
        let l1_provider = OracleL1ChainProvider::new(boot.clone(), oracle.clone());
        let l2_provider = OracleL2ChainProvider::new(boot.clone(), oracle.clone());
        let beacon: OracleBlobProvider<
            CachingOracle<kona_preimage::OracleReader, kona_preimage::HintWriter>,
        > = OracleBlobProvider::new(oracle.clone());
        let celestia_provider = OracleCelestiaProvider::new(oracle.clone());

        ////////////////////////////////////////////////////////////////
        //                   DERIVATION & EXECUTION                   //
        ////////////////////////////////////////////////////////////////

        let namespace_var = var("NAMESPACE").expect("Failed to load namespace");
        let namespace_bytes = hex::decode(namespace_var).expect("Invalid hex");
        let namespace = Namespace::new_v0(&namespace_bytes).expect("Invalid namespace");
        // Create a new derivation driver with the given boot information and oracle.
        let mut driver = DerivationDriver::new(
            boot.as_ref(),
            oracle.as_ref(),
            beacon,
            celestia_provider,
            namespace,
            l1_provider,
            l2_provider.clone(),
        )
        .await?;

        // Run the derivation pipeline until we are able to produce the output root of the claimed
        // L2 block.
        let (number, output_root) = driver
            .produce_output(&boot.rollup_config, &l2_provider, &l2_provider, fpvm_handle_register)
            .await?;

        ////////////////////////////////////////////////////////////////
        //                          EPILOGUE                          //
        ////////////////////////////////////////////////////////////////

        if number != boot.claimed_l2_block_number || output_root != boot.claimed_l2_output_root {
            tracing::error!(
                target: "client",
                "Failed to validate L2 block #{number} with output root {output_root}",
                number = number,
                output_root = output_root
            );
            kona_common::io::print(&alloc::format!(
                "Failed to validate L2 block #{} with output root {}\n",
                number,
                output_root
            ));

            kona_common::io::exit(1);
        }

        tracing::info!(
            target: "client",
            "Successfully validated L2 block #{number} with output root {output_root}",
            number = number,
            output_root = output_root
        );

        kona_common::io::print(&alloc::format!(
            "Successfully validated L2 block #{} with output root {}\n",
            number,
            output_root
        ));

        Ok::<_, anyhow::Error>(())
    })
}
