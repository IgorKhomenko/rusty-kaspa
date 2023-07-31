use crate::tx::{Transaction, TransactionInput, TransactionOutput, TransactionOutputInner};
use wasm_bindgen::prelude::*;

use kaspa_consensus_core::{
    config::params::Params,
    // config::params::{Params, DEVNET_PARAMS, MAINNET_PARAMS, SIMNET_PARAMS, TESTNET_PARAMS},
    constants::*,
    subnets::SUBNETWORK_ID_SIZE,
    // mass::{self, MassCalculator},
};
use kaspa_hashes::HASH_SIZE;

// pub const ECDSA_SIGNATURE_SIZE: u64 = 64;
// pub const SCHNORR_SIGNATURE_SIZE: u64 = 64;
pub const SIGNATURE_SIZE: u64 = 1 + 64 + 1; //1 byte for OP_DATA_65 + 64 (length of signature) + 1 byte for sig hash type

/// MINIMUM_RELAY_TRANSACTION_FEE specifies the minimum transaction fee for a transaction to be accepted to
/// the mempool and relayed. It is specified in sompi per 1kg (or 1000 grams) of transaction mass.
pub(crate) const MINIMUM_RELAY_TRANSACTION_FEE: u64 = 1000;

/// MAXIMUM_STANDARD_TRANSACTION_MASS is the maximum mass allowed for transactions that
/// are considered standard and will therefore be relayed and considered for mining.
pub const MAXIMUM_STANDARD_TRANSACTION_MASS: u64 = 100_000;

/// minimum_required_transaction_relay_fee returns the minimum transaction fee required
/// for a transaction with the passed mass to be accepted into the mempool and relayed.
pub fn minimum_required_transaction_relay_fee(mass: u64) -> u64 {
    // Calculate the minimum fee for a transaction to be allowed into the
    // mempool and relayed by scaling the base fee. MinimumRelayTransactionFee is in
    // sompi/kg so multiply by mass (which is in grams) and divide by 1000 to get
    // minimum sompis.
    let mut minimum_fee = (mass * MINIMUM_RELAY_TRANSACTION_FEE) / 1000;

    if minimum_fee == 0 {
        minimum_fee = MINIMUM_RELAY_TRANSACTION_FEE;
    }

    // Set the minimum fee to the maximum possible value if the calculated
    // fee is not in the valid range for monetary amounts.
    minimum_fee = minimum_fee.min(MAX_SOMPI);

    minimum_fee
}

/// is_transaction_output_dust returns whether or not the passed transaction output
/// amount is considered dust or not based on the configured minimum transaction
/// relay fee.
///
/// Dust is defined in terms of the minimum transaction relay fee. In particular,
/// if the cost to the network to spend coins is more than 1/3 of the minimum
/// transaction relay fee, it is considered dust.
///
/// It is exposed by [MiningManager] for use by transaction generators and wallets.
#[wasm_bindgen(js_name=isTransactionOutputDust)]
pub fn is_transaction_output_dust(transaction_output: &TransactionOutput) -> bool {
    // Unspendable outputs are considered dust.
    //
    // TODO: call script engine when available
    // if txscript.is_unspendable(transaction_output.script_public_key.script()) {
    //     return true
    // }
    // TODO: Remove this code when script engine is available
    if transaction_output.get_script_public_key().script().len() < 33 {
        return true;
    }

    // The total serialized size consists of the output and the associated
    // input script to redeem it. Since there is no input script
    // to redeem it yet, use the minimum size of a typical input script.
    //
    // Pay-to-pubkey bytes breakdown:
    //
    //  Output to pubkey (43 bytes):
    //   8 value, 1 script len, 34 script [1 OP_DATA_32,
    //   32 pubkey, 1 OP_CHECKSIG]
    //
    //  Input (105 bytes):
    //   36 prev outpoint, 1 script len, 64 script [1 OP_DATA_64,
    //   64 sig], 4 sequence
    //
    // The most common scripts are pay-to-pubkey, and as per the above
    // breakdown, the minimum size of a p2pk input script is 148 bytes. So
    // that figure is used.
    // let output = transaction_output.clone().try_into().unwrap();
    let total_serialized_size = transaction_output_serialized_byte_size(transaction_output) + 148;

    // The output is considered dust if the cost to the network to spend the
    // coins is more than 1/3 of the minimum free transaction relay fee.
    // mp.config.MinimumRelayTransactionFee is in sompi/KB, so multiply
    // by 1000 to convert to bytes.
    //
    // Using the typical values for a pay-to-pubkey transaction from
    // the breakdown above and the default minimum free transaction relay
    // fee of 1000, this equates to values less than 546 sompi being
    // considered dust.
    //
    // The following is equivalent to (value/total_serialized_size) * (1/3) * 1000
    // without needing to do floating point math.
    //
    // Since the multiplication may overflow a u64, 2 separate calculation paths
    // are considered to avoid overflowing.
    let value = transaction_output.get_value();
    match value.checked_mul(1000) {
        Some(value_1000) => value_1000 / (3 * total_serialized_size) < MINIMUM_RELAY_TRANSACTION_FEE,
        None => (value as u128 * 1000 / (3 * total_serialized_size as u128)) < MINIMUM_RELAY_TRANSACTION_FEE as u128,
    }
}

// transaction_estimated_serialized_size is the estimated size of a transaction in some
// serialization. This has to be deterministic, but not necessarily accurate, since
// it's only used as the size component in the transaction and block mass limit
// calculation.
pub fn transaction_serialized_byte_size(tx: &Transaction) -> u64 {
    let inner = tx.inner();

    let mut size: u64 = 0;
    size += 2; // Tx version (u16)
    size += 8; // Number of inputs (u64)
    let inputs_size: u64 = inner.inputs.iter().map(transaction_input_serialized_byte_size).sum();
    size += inputs_size;

    size += 8; // number of outputs (u64)
    let outputs_size: u64 = inner.outputs.iter().map(transaction_output_serialized_byte_size).sum();
    size += outputs_size;

    size += 8; // lock time (u64)
    size += SUBNETWORK_ID_SIZE as u64;
    size += 8; // gas (u64)
    size += HASH_SIZE as u64; // payload hash

    size += 8; // length of the payload (u64)
    size += inner.payload.len() as u64;
    size
}

pub const fn blank_transaction_serialized_byte_size() -> u64 {
    let mut size: u64 = 0;
    size += 2; // Tx version (u16)
    size += 8; // Number of inputs (u64)
               // ~ skip input size for blank tx
    size += 8; // number of outputs (u64)
               // ~ skip output size for blank tx
    size += 8; // lock time (u64)
    size += SUBNETWORK_ID_SIZE as u64;
    size += 8; // gas (u64)
    size += HASH_SIZE as u64; // payload hash

    size += 8; // length of the payload (u64)
               // ~ skip payload size for blank tx
    size
}

fn transaction_input_serialized_byte_size(input: &TransactionInput) -> u64 {
    let mut size = 0;
    size += outpoint_estimated_serialized_size();

    size += 8; // length of signature script (u64)
    size += input.inner().signature_script.len() as u64;

    size += 8; // sequence (uint64)
    size
}

const fn outpoint_estimated_serialized_size() -> u64 {
    let mut size: u64 = 0;
    size += HASH_SIZE as u64; // Previous tx ID
    size += 4; // Index (u32)
    size
}

pub fn transaction_output_serialized_byte_size(output: &TransactionOutput) -> u64 {
    transaction_output_serialized_byte_size_for_inner(&output.inner())
}

pub fn transaction_output_serialized_byte_size_for_inner(output_inner: &TransactionOutputInner) -> u64 {
    let mut size: u64 = 0;
    size += 8; // value (u64)
    size += 2; // output.ScriptPublicKey.Version (u16)
    size += 8; // length of script public key (u64)
    size += output_inner.script_public_key.script().len() as u64;
    size
}

pub struct MassCalculator {
    mass_per_tx_byte: u64,
    mass_per_script_pub_key_byte: u64,
    mass_per_sig_op: u64,
}

impl MassCalculator {
    // pub fn new(mass_per_tx_byte: u64, mass_per_script_pub_key_byte: u64, mass_per_sig_op: u64) -> Self {
    //     Self { mass_per_tx_byte, mass_per_script_pub_key_byte, mass_per_sig_op }
    // }

    pub fn new(params: Params) -> Self {
        Self {
            mass_per_tx_byte: params.mass_per_tx_byte,
            mass_per_script_pub_key_byte: params.mass_per_script_pub_key_byte,
            mass_per_sig_op: params.mass_per_sig_op,
        }
    }

    pub fn calc_mass_for_tx(&self, tx: &Transaction) -> u64 {
        self.calc_serialized_mass_for_tx(tx)
            + self.calc_mass_for_outputs(&tx.inner().outputs)
            + self.calc_mass_for_inputs(&tx.inner().inputs)

        // let size = transaction_estimated_serialized_size(tx);
        // let mass_for_size = size * self.mass_per_tx_byte;
        // let total_script_public_key_size: u64 = tx
        //     .outputs
        //     .iter()
        //     .map(|output| 2 /* script public key version (u16) */ + output.script_public_key.script().len() as u64)
        //     .sum();
        // let total_script_public_key_mass = total_script_public_key_size * self.mass_per_script_pub_key_byte;

        // let total_sigops: u64 = tx.inputs.iter().map(|input| input.sig_op_count as u64).sum();
        // let total_sigops_mass = total_sigops * self.mass_per_sig_op;

        // mass_for_size + total_script_public_key_mass + total_sigops_mass
    }

    // ==================================================================
    // added for wallet tx generation

    pub fn calc_mass_for_payload(&self, payload_byte_size: usize) -> u64 {
        payload_byte_size as u64 * self.mass_per_tx_byte
    }

    pub fn blank_transaction_serialized_mass(&self) -> u64 {
        blank_transaction_serialized_byte_size() * self.mass_per_tx_byte
    }

    fn calc_serialized_mass_for_tx(&self, tx: &Transaction) -> u64 {
        transaction_serialized_byte_size(tx) * self.mass_per_tx_byte
    }

    // pub fn calc_

    pub fn calc_mass_for_outputs(&self, outputs: &[TransactionOutput]) -> u64 {
        let total_script_public_key_size: u64 = outputs
            .iter()
            .map(|output| 2 /* script public key version (u16) */ + output.inner().script_public_key.script().len() as u64)
            .sum();
        total_script_public_key_size * self.mass_per_script_pub_key_byte
    }

    pub fn calc_mass_for_inputs(&self, inputs: &[TransactionInput]) -> u64 {
        inputs.iter().map(|input| input.inner().sig_op_count as u64).sum::<u64>() * self.mass_per_sig_op
    }

    pub fn calc_mass_for_output(&self, output: &TransactionOutput) -> u64 {
        self.mass_per_script_pub_key_byte * (2 + output.inner().script_public_key.script().len() as u64)
    }

    pub fn calc_mass_for_input(&self, input: &TransactionInput) -> u64 {
        input.inner().sig_op_count as u64 * self.mass_per_sig_op
    }

    pub fn calc_signature_mass(&self, minimum_signatures: u16) -> u64 {
        let minimum_signatures = std::cmp::max(1, minimum_signatures);
        SIGNATURE_SIZE * self.mass_per_tx_byte * minimum_signatures as u64
    }

    pub fn calc_signature_mass_for_inputs(&self, number_of_inputs: usize, minimum_signatures: u16) -> u64 {
        let minimum_signatures = std::cmp::max(1, minimum_signatures);
        SIGNATURE_SIZE * self.mass_per_tx_byte * minimum_signatures as u64 * number_of_inputs as u64
    }

    pub fn calc_minium_tx_relay_fee(&self, tx: &Transaction, minimum_signatures: u16) -> u64 {
        let mass = self.calc_mass_for_tx(tx) + self.calc_signature_mass_for_inputs(tx.inner().inputs.len(), minimum_signatures);
        minimum_required_transaction_relay_fee(mass)
    }
}

// pub fn calculate_mass(tx: &Transaction, params: &Params, estimate_signature_mass: bool, minimum_signatures: u16) -> u64 {
//     let mass_calculator = MassCalculator::new(params.clone());
//     let mass = mass_calculator.calc_mass_for_tx(tx);

//     if !estimate_signature_mass {
//         return mass;
//     }

//     // //TODO: remove this sig_op_count mass calculation
//     // let sig_op_count = 1;
//     // mass += (sig_op_count * tx.inner().inputs.len() as u64) * params.mass_per_sig_op;

//     let signature_mass = transaction_estimate_signature_mass(tx, params, minimum_signatures);
//     mass + signature_mass
// }

// pub fn transaction_estimate_signature_mass(tx: &Transaction, params: &Params, mut minimum_signatures: u16) -> u64 {
//     //let signature_script_size = 66; //params.max_signature_script_len;
//     // let size = if ecdsa {
//     //     ECDSA_SIGNATURE_SIZE
//     // }else{
//     //     SCHNORR_SIGNATURE_SIZE
//     // };
//     if minimum_signatures < 1 {
//         minimum_signatures = 1;
//     }
//     //TODO create redeem script to calculate mass
//     tx.inner().inputs.len() as u64 * SIGNATURE_SIZE * params.mass_per_tx_byte * minimum_signatures as u64
// }

// pub fn calculate_minimum_transaction_fee(
//     tx: &Transaction,
//     params: &Params,
//     estimate_signature_mass: bool,
//     minimum_signatures: u16,
// ) -> u64 {
//     crate::tx::minimum_required_transaction_relay_fee(calculate_mass(tx, params, estimate_signature_mass, minimum_signatures))
// }
