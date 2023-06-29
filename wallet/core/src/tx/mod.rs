pub mod input;
pub mod mtx;
pub mod outpoint;
pub mod output;
pub mod payment;
pub mod transaction;
pub mod txscript;
pub mod virtual_transaction;

pub use input::*;
pub use mtx::*;
pub use outpoint::*;
pub use output::*;
pub use payment::*;
pub use transaction::*;
pub use txscript::*;
pub use virtual_transaction::*;

use crate::result::Result;
use crate::utils::*;
use crate::utxo::*;
use kaspa_addresses::Address;
use kaspa_consensus_core::hashing::sighash::calc_schnorr_signature_hash;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValues;
use kaspa_consensus_core::hashing::sighash_type::SIG_HASH_ALL;
use kaspa_consensus_core::networktype::NetworkType;
use kaspa_consensus_core::subnets::SubnetworkId;
use kaspa_consensus_core::tx::SignableTransaction;
use kaspa_txscript::pay_to_address_script;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use workflow_log::log_trace;

pub fn script_hashes(mut mutable_tx: SignableTransaction) -> Result<Vec<kaspa_hashes::Hash>> {
    let mut list = vec![];
    for i in 0..mutable_tx.tx.inputs.len() {
        mutable_tx.tx.inputs[i].sig_op_count = 1;
    }

    let mut reused_values = SigHashReusedValues::new();
    for i in 0..mutable_tx.tx.inputs.len() {
        let sig_hash = calc_schnorr_signature_hash(&mutable_tx.as_verifiable(), i, SIG_HASH_ALL, &mut reused_values);
        list.push(sig_hash);
    }
    Ok(list)
}

#[wasm_bindgen(js_name=createTransaction)]
pub fn js_create_transaction(
    sig_op_count: u8,
    ctx: &mut UtxoSelectionContext,
    outputs: JsValue,
    change_address: &Address,
    minimum_signatures: u16,
    priority_fee: Option<u64>,
    payload: Option<Vec<u8>>,
) -> crate::Result<MutableTransaction> {
    let outputs: PaymentOutputs = outputs.try_into()?;

    create_transaction(sig_op_count, ctx, &outputs, change_address, minimum_signatures, priority_fee, payload)
}

pub fn create_transaction(
    sig_op_count: u8,
    ctx: &mut UtxoSelectionContext,
    outputs: &PaymentOutputs,
    change_address: &Address,
    minimum_signatures: u16,
    priority_fee: Option<u64>,
    payload: Option<Vec<u8>>,
) -> crate::Result<MutableTransaction> {
    let entries = ctx.selected_entries();

    let utxos = entries.iter().map(|reference| reference.utxo.clone()).collect::<Vec<Arc<UtxoEntry>>>();

    // let prev_tx_id = TransactionId::default();
    let mut total_input_amount = 0;
    let mut entries = vec![];

    let inputs = utxos
        .iter()
        .enumerate()
        .map(|(sequence, utxo)| {
            total_input_amount += utxo.utxo_entry.amount;
            entries.push(utxo.as_ref().clone());
            TransactionInput::new(utxo.outpoint.clone(), vec![], sequence as u64, sig_op_count)
        })
        .collect::<Vec<TransactionInput>>();

    let priority_fee = priority_fee.unwrap_or(0);
    if priority_fee > total_input_amount {
        return Err(format!("priority fee({priority_fee}) > amount({total_input_amount})").into());
    }

    let mut outputs_ = vec![];
    for output in &outputs.outputs {
        outputs_.push(TransactionOutput::new(output.amount, &pay_to_address_script(&output.address)));
    }

    let tx = Transaction::new(
        0,
        inputs,
        outputs_,
        0,
        SubnetworkId::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        0,
        payload.unwrap_or(vec![]),
    )?;

    let mtx = MutableTransaction::new(&tx, &entries.into());
    adjust_transaction_for_fee(&mtx, change_address, minimum_signatures, Some(priority_fee))?;

    Ok(mtx)
}

#[wasm_bindgen(js_name=adjustTransactionForFee)]
pub fn adjust_transaction_for_fee(
    mtx: &MutableTransaction,
    change_address: &Address,
    minimum_signatures: u16,
    priority_fee: Option<u64>,
) -> crate::Result<bool> {
    let total_input_amount = mtx.total_input_amount()?;
    let mut total_output_amount = mtx.total_output_amount()?;
    let priority_fee = priority_fee.unwrap_or(0);

    let amount_after_priority_fee = total_input_amount - priority_fee;
    if total_output_amount > amount_after_priority_fee {
        return Err(format!("total_amount({total_output_amount}) > amount_after_priority_fee({amount_after_priority_fee})").into());
    }

    let tx = (*mtx.tx()).clone();

    let change = amount_after_priority_fee - total_output_amount;
    let mut change_output_opt = None;
    if change > 0 {
        let change_output = TransactionOutput::new(change, &pay_to_address_script(change_address));
        if !change_output.is_dust() {
            total_output_amount += change;
            change_output_opt = Some(change_output.clone());
            tx.inner().outputs.push(change_output);
        }
    }

    let params = get_consensus_params_by_address(change_address);
    let minimum_fee = calculate_minimum_transaction_fee(&tx, &params, true, minimum_signatures);
    let total_fee = minimum_fee + priority_fee;
    log_trace!("minimum_fee: {minimum_fee}");
    log_trace!("priority_fee: {priority_fee}");
    log_trace!("total_fee: {total_fee}");

    let fee = total_input_amount - total_output_amount;
    log_trace!("fee: {fee}");

    //if tx fee is less than required minimum fee + priority_fee
    if fee < total_fee {
        let fee_difference = total_fee - fee;

        // if there is no change output or change cant fullfill minimum required fee
        if change_output_opt.is_none() || change < fee_difference {
            return Err(format!("total_fee({total_fee}) > tx fee({fee})").into());
        }

        let change_output = change_output_opt.unwrap();

        let new_change = change - fee_difference;
        change_output.inner().value = new_change;

        if change_output.is_dust() {
            let _change_output = tx.inner().outputs.pop().unwrap();
        }
    }

    Ok(true)
}

/// Calculate the minimum transaction fee. Transaction fee is derived from the
///
#[wasm_bindgen(js_name = "minimumTransactionFee")]
pub fn minimum_transaction_fee(tx: &Transaction, network_type: NetworkType, minimum_signatures: u16) -> u64 {
    let params = get_consensus_params_by_network(&network_type);
    calculate_minimum_transaction_fee(tx, &params, true, minimum_signatures)
}

/// Calculate transaction mass. Transaction mass is used in the fee calculation.
#[wasm_bindgen(js_name = "calculateTransactionMass")]
pub fn calculate_mass_js(
    tx: &Transaction,
    network_type: NetworkType,
    estimate_signature_mass: bool,
    minimum_signatures: u16,
) -> Result<u64> {
    let params = get_consensus_params_by_network(&network_type);
    Ok(calculate_mass(tx, &params, estimate_signature_mass, minimum_signatures))
}
