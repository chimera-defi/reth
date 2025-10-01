use alloy_primitives::B256;

fn main() {
    let current = B256::ZERO;
    let range_size = 0x1000000000000000; // Default range size
    
    println!("Current: {:?}", current);
    println!("Range size: 0x{:x}", range_size);
    
    // Test the increment logic
    let mut hash_bytes = current.as_slice().to_owned();
    let mut carry = range_size;
    
    for i in (0..32).rev() {
        let (new_val, new_carry) = hash_bytes[i].overflowing_add(carry as u8);
        hash_bytes[i] = new_val;
        carry = if new_carry { 1 } else { 0 };
        if carry == 0 {
            break;
        }
    }
    
    println!("Carry after processing: {}", carry);
    println!("Result bytes: {:?}", hash_bytes);
    
    let result = B256::from_slice(&hash_bytes);
    println!("Result: {:?}", result);
    println!("Result > current: {}", result > current);
}