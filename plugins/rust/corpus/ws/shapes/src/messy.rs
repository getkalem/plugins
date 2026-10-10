//! A module left unformatted on purpose: the plugin's formatting is
//! checked on it (rustfmt, with the workspace's `rustfmt.toml`).

/// A point.
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct Point{pub x:f64,pub y:f64}

impl Point{
/// A point at `x`, `y`.
pub fn new(x:f64,y:f64)->Self{Point{x:x,y:y}}
    /// Its distance to `other`.
        pub fn distance(&self,other:&Point)->f64{let dx=self.x-other.x;let dy=self.y-other.y;(dx*dx+dy*dy).sqrt()}
}
