//! Shapes with an area: a trait, two structs and a macro, used by the
//! `app` crate of the workspace.

/// A shape with an area.
pub trait Area {
    /// The area, in square units.
    fn area(&self) -> f64;
}

/// A circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    /// Its radius.
    pub radius: f64,
}

impl Circle {
    /// A circle of radius `radius`.
    pub fn new(radius: f64) -> Self {
        Self { radius }
    }

    /// The circle `factor` times as wide.
    pub fn scaled(self, factor: f64) -> Self {
        Self::new(self.radius * factor)
    }
}

impl Area for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.radius * self.radius
    }
}

/// A square.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Square {
    /// The length of its side.
    pub side: f64,
}

impl Area for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

/// A square of side `$side`: `square!(2.0)`.
#[macro_export]
macro_rules! square {
    ($side:expr) => {
        $crate::Square { side: $side }
    };
}
