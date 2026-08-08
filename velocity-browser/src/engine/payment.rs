use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct PaymentItem {
    pub label: String,
    pub amount_value: f64,
    pub currency: String,
}

#[derive(Debug, Clone)]
pub struct ShippingOption {
    pub id: String,
    pub label: String,
    pub amount_value: f64,
    pub currency: String,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct PaymentMethodFilter {
    pub supported_methods: Vec<String>,
    pub supported_types: Vec<PaymentMethodType>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaymentMethodType {
    BasicCard,
    CreditTransfer,
    DebitTransfer,
}

#[derive(Debug, Clone)]
pub struct PaymentAddress {
    pub recipient: String,
    pub address_line: Vec<String>,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
    pub phone: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PaymentValidationErrors {
    pub errors: Vec<(String, String)>,
}

pub struct PaymentRequestEngine {
    pub merchant_name: String,
    pub items: Vec<PaymentItem>,
    pub shipping_options: Vec<ShippingOption>,
    pub method_filters: Vec<PaymentMethodFilter>,
    pub selected_shipping_id: Option<String>,
    pub shipping_address: Option<PaymentAddress>,
    pub is_resolved: bool,
    pub total_currency: String,
    pub require_shipping: bool,
}

impl PaymentRequestEngine {
    pub fn new(merchant_name: &str) -> Self {
        Self {
            merchant_name: merchant_name.to_string(),
            items: Vec::new(),
            shipping_options: Vec::new(),
            method_filters: Vec::new(),
            selected_shipping_id: None,
            shipping_address: None,
            is_resolved: false,
            total_currency: "USD".to_string(),
            require_shipping: false,
        }
    }

    pub fn add_item(&mut self, label: &str, amount: f64, currency: &str) {
        self.items.push(PaymentItem {
            label: label.to_string(),
            amount_value: amount,
            currency: currency.to_string(),
        });
    }

    pub fn remove_item(&mut self, label: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.label != label);
        self.items.len() < before
    }

    pub fn add_shipping_option(&mut self, id: &str, label: &str, amount: f64, currency: &str) {
        let first = self.shipping_options.is_empty();
        self.shipping_options.push(ShippingOption {
            id: id.to_string(),
            label: label.to_string(),
            amount_value: amount,
            currency: currency.to_string(),
            selected: false,
        });
        if first {
            self.selected_shipping_id = Some(id.to_string());
            if let Some(opt) = self.shipping_options.first_mut() {
                opt.selected = true;
            }
        }
    }

    pub fn select_shipping(&mut self, option_id: &str) -> bool {
        let mut found = false;
        for opt in &mut self.shipping_options {
            if opt.id == option_id {
                opt.selected = true;
                found = true;
            } else {
                opt.selected = false;
            }
        }
        if found {
            self.selected_shipping_id = Some(option_id.to_string());
        }
        found
    }

    pub fn add_method_filter(&mut self, methods: Vec<String>, types: Vec<PaymentMethodType>) {
        self.method_filters.push(PaymentMethodFilter {
            supported_methods: methods,
            supported_types: types,
        });
    }

    pub fn set_shipping_address(&mut self, address: PaymentAddress) {
        self.shipping_address = Some(address);
    }

    /// Compute the subtotal of all items.
    pub fn subtotal(&self) -> f64 {
        self.items.iter().map(|i| i.amount_value).sum()
    }

    /// Get the selected shipping cost.
    pub fn shipping_cost(&self) -> f64 {
        self.shipping_options.iter()
            .find(|o| o.selected)
            .map(|o| o.amount_value)
            .unwrap_or(0.0)
    }

    /// Compute total: subtotal + shipping.
    pub fn total(&self) -> f64 {
        self.subtotal() + self.shipping_cost()
    }

    /// Validate the payment request.
    pub fn validate(&self) -> PaymentValidationErrors {
        let mut errors = Vec::new();
        if self.items.is_empty() {
            errors.push(("items".to_string(), "At least one item is required".to_string()));
        }
        for item in &self.items {
            if item.amount_value < 0.0 {
                errors.push((item.label.clone(), "Amount cannot be negative".to_string()));
            }
            if item.currency.is_empty() {
                errors.push((item.label.clone(), "Currency must be specified".to_string()));
            }
        }
        if self.require_shipping && self.shipping_address.is_none() {
            errors.push(("shipping".to_string(), "Shipping address is required".to_string()));
        }
        if self.require_shipping && self.shipping_options.is_empty() {
            errors.push(("shipping_options".to_string(), "At least one shipping option is required".to_string()));
        }
        for opt in &self.shipping_options {
            if opt.amount_value < 0.0 {
                errors.push((opt.id.clone(), "Shipping cost cannot be negative".to_string()));
            }
        }
        PaymentValidationErrors { errors }
    }

    pub fn is_valid(&self) -> bool {
        self.validate().errors.is_empty()
    }

    pub fn show(&mut self) -> Result<String, String> {
        let validation = self.validate();
        if !validation.errors.is_empty() {
            let msgs: Vec<String> = validation.errors.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
            return Err(format!("Payment validation failed: {}", msgs.join("; ")));
        }
        self.is_resolved = true;
        Ok(format!(
            "Payment authorized for merchant '{}'. Total: {:.2} {} (items: {:.2}, shipping: {:.2})",
            self.merchant_name,
            self.total(),
            self.total_currency,
            self.subtotal(),
            self.shipping_cost(),
        ))
    }

    pub fn export_payment_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        if self.is_resolved {
            triples.push(NdaTriple::new(session_id, 240, &self.merchant_name));
            triples.push(NdaTriple::new(session_id, 241, &format!("{:.2}", self.total())));
        }
        triples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtotal_and_total() {
        let mut engine = PaymentRequestEngine::new("TestShop");
        engine.add_item("Widget", 9.99, "USD");
        engine.add_item("Gadget", 19.99, "USD");
        assert!((engine.subtotal() - 29.98).abs() < 1e-6);
        assert!((engine.total() - 29.98).abs() < 1e-6); // no shipping
    }

    #[test]
    fn test_shipping_cost() {
        let mut engine = PaymentRequestEngine::new("TestShop");
        engine.add_item("Item", 50.0, "USD");
        engine.add_shipping_option("std", "Standard", 5.99, "USD");
        engine.add_shipping_option("exp", "Express", 14.99, "USD");
        assert!((engine.shipping_cost() - 5.99).abs() < 1e-6); // first auto-selected
        engine.select_shipping("exp");
        assert!((engine.shipping_cost() - 14.99).abs() < 1e-6);
        assert!((engine.total() - 64.99).abs() < 1e-6);
    }

    #[test]
    fn test_remove_item() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("A", 10.0, "USD");
        engine.add_item("B", 20.0, "USD");
        assert!(engine.remove_item("A"));
        assert!((engine.subtotal() - 20.0).abs() < 1e-6);
        assert!(!engine.remove_item("C")); // not found
    }

    #[test]
    fn test_validate_empty_items() {
        let engine = PaymentRequestEngine::new("Shop");
        assert!(!engine.is_valid());
        let errors = engine.validate();
        assert!(errors.errors.iter().any(|(k, _)| k == "items"));
    }

    #[test]
    fn test_validate_negative_amount() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("Bad", -5.0, "USD");
        let errors = engine.validate();
        assert!(errors.errors.iter().any(|(k, _)| k == "Bad"));
    }

    #[test]
    fn test_validate_shipping_required() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("Item", 10.0, "USD");
        engine.require_shipping = true;
        let errors = engine.validate();
        assert!(errors.errors.iter().any(|(k, _)| k == "shipping"));
    }

    #[test]
    fn test_show_success() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("Item", 25.0, "USD");
        let result = engine.show();
        assert!(result.is_ok());
        assert!(engine.is_resolved);
    }

    #[test]
    fn test_show_validation_failure() {
        let mut engine = PaymentRequestEngine::new("Shop");
        let result = engine.show();
        assert!(result.is_err());
    }

    #[test]
    fn test_export_nda_only_when_resolved() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("X", 10.0, "USD");
        assert!(engine.export_payment_nda("s1").is_empty());
        engine.show().ok();
        assert!(!engine.export_payment_nda("s1").is_empty());
    }

    #[test]
    fn select_nonexistent_shipping_returns_false() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_shipping_option("std", "Standard", 5.0, "USD");
        assert!(!engine.select_shipping("nonexistent"));
        // select_shipping deselects all when not found
        assert!((engine.shipping_cost() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn validate_empty_currency() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("Bad", 10.0, "");
        let errors = engine.validate();
        assert!(errors.errors.iter().any(|(k, _)| k == "Bad"));
    }

    #[test]
    fn validate_negative_shipping_cost() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_item("Item", 10.0, "USD");
        engine.add_shipping_option("bad", "Bad Ship", -5.0, "USD");
        let errors = engine.validate();
        assert!(errors.errors.iter().any(|(k, _)| k == "bad"));
    }

    #[test]
    fn show_success_message_includes_total() {
        let mut engine = PaymentRequestEngine::new("MyShop");
        engine.add_item("A", 10.0, "USD");
        engine.add_shipping_option("s", "Std", 5.0, "USD");
        let msg = engine.show().unwrap();
        assert!(msg.contains("MyShop"));
        assert!(msg.contains("15.00"));
        assert!(engine.is_resolved);
    }

    #[test]
    fn shipping_cost_zero_when_none_selected() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_shipping_option("a", "A", 10.0, "USD");
        // Selecting nonexistent deselects all options
        engine.select_shipping("nonexistent");
        assert!((engine.shipping_cost() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn add_method_filter_stores_filters() {
        let mut engine = PaymentRequestEngine::new("Shop");
        engine.add_method_filter(
            vec!["basic-card".into()],
            vec![PaymentMethodType::BasicCard],
        );
        assert_eq!(engine.method_filters.len(), 1);
        assert_eq!(engine.method_filters[0].supported_methods[0], "basic-card");
    }
}
