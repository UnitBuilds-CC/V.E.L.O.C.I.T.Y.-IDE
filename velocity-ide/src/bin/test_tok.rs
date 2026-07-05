use std::path::Path;
use velocity_ide::tokenizer::Tokenizer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tok_path = Path::new("models/qwen-coder-0.5b/tokenizer.json");
    let tokenizer = Tokenizer::from_file(&tok_path)?;
    
    let prompt = "How do I verify a Paystack webhook signature in Node.js?";
    let tokens = tokenizer.encode(prompt, false);
    println!("Tokens: {:?}", tokens);
    for &t in &tokens {
        println!("Token {}: {:?}", t, tokenizer.decode_token(t));
    }
    Ok(())
}
