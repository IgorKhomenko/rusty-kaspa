use super::{UtxoContext, UtxoEntryReference};
use crate::imports::*;

pub struct UtxoIterator {
    utxo_context: UtxoContext,
    cursor: usize,
}

impl UtxoIterator {
    pub fn new(utxo_context: &UtxoContext) -> Self {
        Self { utxo_context: utxo_context.clone(), cursor: 0 }
    }
}


impl Iterator for UtxoIterator {
    type Item = UtxoEntryReference;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.utxo_context.inner.lock().unwrap().mature.get(self.cursor).cloned();
        self.cursor += 1;
        entry
    }
}

