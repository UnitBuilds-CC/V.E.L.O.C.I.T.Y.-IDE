use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct PaymentItem {
    pub label: String,
    pub amount_value: f64,
    pub currency: String,
}

pub struct PaymentRequestEngine {
    pub merchant_name: String,
    pub items: Vec<PaymentItem>,
    pub is_resolved: bool,
}

impl PaymentRequestEngine {
    pub fn new(merchant_name: &str) -> Self {
        Self {
            merchant_name: merchant_name.to_string(),
            items: Vec::new(),
            is_resolved: false,
        }
    }

    pub fn add_item(&mut self, label: &str, amount: f64, currency: &str) {
        self.items.push(PaymentItem {
            label: label.to_string(),
            amount_value: amount,
            currency: currency.to_string(),
        });
    }

    pub fn show(&mut self) -> Result<String, String> {
        self.is_resolved = true;
        Ok(format!("Payment authorized for merchant '{}'", self.merchant_name))
    }

    pub fn export_payment_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        if self.is_resolved {
            triples.push(NdaTriple::new(session_id, 240, &self.merchant_name));
        }
        triples
    }
}
