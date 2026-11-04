//! Géométrie de base de MakerCraft (couche L0).
//!
//! Types neutres partagés par toutes les extensions. Ils seront unifiés avec `cadcraft-geom`
//! et `solidcraft-geom` au branchement des extensions dao et cao (conversions `From`).
//! Unités : millimètres, f64.

use std::ops::{Add, Mul, Neg, Sub};

/// Point ou vecteur 2D.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Self = Self::new(0.0, 0.0);

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// Produit vectoriel (composante z) : > 0 si `o` est à gauche de `self`.
    pub fn cross(self, o: Self) -> f64 {
        self.x * o.y - self.y * o.x
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Point ou vecteur 3D.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Self) -> Self {
        Self::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

macro_rules! impl_ops {
    ($t:ident, $($f:ident),+) => {
        impl Add for $t { type Output = Self; fn add(self, o: Self) -> Self { Self { $($f: self.$f + o.$f),+ } } }
        impl Sub for $t { type Output = Self; fn sub(self, o: Self) -> Self { Self { $($f: self.$f - o.$f),+ } } }
        impl Mul<f64> for $t { type Output = Self; fn mul(self, k: f64) -> Self { Self { $($f: self.$f * k),+ } } }
        impl Neg for $t { type Output = Self; fn neg(self) -> Self { Self { $($f: -self.$f),+ } } }
    };
}
impl_ops!(Vec2, x, y);
impl_ops!(Vec3, x, y, z);

/// Boîte englobante 2D alignée sur les axes. Vide tant qu'aucun point n'y est ajouté.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds2 {
    pub min: Vec2,
    pub max: Vec2,
}

impl Default for Bounds2 {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Bounds2 {
    pub const EMPTY: Self = Self { min: Vec2::new(f64::INFINITY, f64::INFINITY), max: Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY) };

    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }

    /// Ajoute un point ; les points non finis (NaN, ∞) sont ignorés.
    pub fn include(&mut self, p: Vec2) {
        if !p.is_finite() {
            return;
        }
        self.min = Vec2::new(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = Vec2::new(self.max.x.max(p.x), self.max.y.max(p.y));
    }

    pub fn size(&self) -> Vec2 {
        if self.is_empty() { Vec2::ZERO } else { self.max - self.min }
    }
}

impl FromIterator<Vec2> for Bounds2 {
    fn from_iter<I: IntoIterator<Item = Vec2>>(iter: I) -> Self {
        let mut b = Self::EMPTY;
        for p in iter {
            b.include(p);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_ops() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!(Vec2::new(1.0, 0.0).cross(Vec2::new(0.0, 1.0)), 1.0);
        assert_eq!(Vec3::new(1.0, 0.0, 0.0).cross(Vec3::new(0.0, 1.0, 0.0)), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(a - a, Vec2::ZERO);
    }

    #[test]
    fn bounds_ignore_non_finite() {
        let b: Bounds2 = [Vec2::new(0.0, 0.0), Vec2::new(f64::NAN, 1.0), Vec2::new(10.0, 5.0)].into_iter().collect();
        assert_eq!(b.size(), Vec2::new(10.0, 5.0));
        assert!(Bounds2::EMPTY.is_empty());
        assert_eq!(Bounds2::EMPTY.size(), Vec2::ZERO);
    }
}
