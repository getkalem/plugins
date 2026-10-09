//! Adds the areas of a circle and a square from the `shapes` crate.

use shapes::{Area, Circle, square};

/// The sum of the areas of `shapes`.
fn total(shapes: &[&dyn Area]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

fn main() {
    let circle = Circle::new(1.0).scaled(2.0);
    let tile = square!(3.0);
    println!("{:.2}", total(&[&circle, &tile]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_adds_the_areas() {
        let tile = square!(2.0);
        assert_eq!(total(&[&tile, &tile]), 8.0);
    }
}
