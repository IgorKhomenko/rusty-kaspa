use crate::imports::*;
use crate::result::Result;
use crate::tx::generator as native;
use crate::wasm::PrivateKeyArrayT;
use kaspa_consensus_client::Transaction;
use kaspa_wallet_keys::privatekey::PrivateKey;
use kaspa_wrpc_wasm::RpcClient;

/// @category Wallet SDK
#[wasm_bindgen(inspectable)]
pub struct PendingTransaction {
    inner: native::PendingTransaction,
}

#[wasm_bindgen]
impl PendingTransaction {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }

    #[wasm_bindgen(getter, js_name = paymentAmount)]
    pub fn payment_value(&self) -> JsValue {
        if let Some(payment_value) = self.inner.payment_value() {
            BigInt::from(payment_value).into()
        } else {
            JsValue::UNDEFINED
        }
    }

    #[wasm_bindgen(getter, js_name = changeAmount)]
    pub fn change_value(&self) -> BigInt {
        BigInt::from(self.inner.change_value())
    }

    #[wasm_bindgen(getter, js_name = feeAmount)]
    pub fn fees(&self) -> BigInt {
        BigInt::from(self.inner.fees())
    }

    #[wasm_bindgen(getter, js_name = aggregateInputAmount)]
    pub fn aggregate_input_value(&self) -> BigInt {
        BigInt::from(self.inner.aggregate_input_value())
    }

    #[wasm_bindgen(getter, js_name = aggregateOutputAmount)]
    pub fn aggregate_output_value(&self) -> BigInt {
        BigInt::from(self.inner.aggregate_output_value())
    }

    #[wasm_bindgen(getter, js_name = "type")]
    pub fn kind(&self) -> String {
        if self.inner.is_batch() {
            "batch".to_string()
        } else {
            "final".to_string()
        }
    }

    #[wasm_bindgen(getter)]
    pub fn addresses(&self) -> Array {
        self.inner.addresses().iter().map(|address| JsValue::from(address.to_string())).collect()
    }

    #[wasm_bindgen(js_name = getUtxoEntries)]
    pub fn get_utxo_entries(&self) -> Array {
        self.inner.utxo_entries().values().map(|utxo_entry| JsValue::from(utxo_entry.clone())).collect()
    }

    /// Sign transaction with supplied [`Array`] or [`PrivateKey`] or an array of
    /// raw private key bytes (encoded as `Uint8Array` or as hex strings)
    pub fn sign(&self, js_value: PrivateKeyArrayT) -> Result<()> {
        if let Ok(keys) = js_value.dyn_into::<Array>() {
            let keys = keys
                .iter()
                .map(PrivateKey::try_cast_from)
                .collect::<std::result::Result<Vec<_>, kaspa_wallet_keys::error::Error>>()?;
            let mut keys = keys.iter().map(|key| key.as_ref().secret_bytes()).collect::<Vec<_>>();
            self.inner.try_sign_with_keys(&keys)?;
            keys.zeroize();
            Ok(())
        } else {
            Err(Error::custom("Please supply an array of keys"))
        }
    }

    /// Submit transaction to the supplied [`RpcClient`]
    pub async fn submit(&self, wasm_rpc_client: &RpcClient) -> Result<String> {
        let rpc: Arc<DynRpcApi> = wasm_rpc_client.client().clone();
        let txid = self.inner.try_submit(&rpc).await?;
        Ok(txid.to_string())
    }

    /// Returns encapsulated network [`Transaction`]
    #[wasm_bindgen(getter)]
    pub fn transaction(&self) -> Result<Transaction> {
        Ok(Transaction::from_cctx_transaction(&self.inner.transaction(), self.inner.utxo_entries()))
    }
}

impl From<native::PendingTransaction> for PendingTransaction {
    fn from(pending_transaction: native::PendingTransaction) -> Self {
        Self { inner: pending_transaction }
    }
}
