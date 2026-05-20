/// Validation program for the LLVM-in-Rust rustc codegen backend.
///
/// This exercises a broad set of MIR constructs so CI can observe which
/// ones the nightly backend handles vs. which ones produce diagnostics.

// Basic arithmetic
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn mul_sub(a: i32, b: i32, c: i32) -> i32 {
    a * b - c
}

// Struct definition and field access
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    fn distance_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }
}

// Enum with match
enum Direction {
    North,
    South,
    East,
    West,
}

fn direction_label(d: Direction) -> &'static str {
    match d {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west",
    }
}

fn main() {
    // Arithmetic
    let sum = add(3, 4);
    let result = mul_sub(5, 6, 7);
    println!("add(3,4)={sum}, mul_sub(5,6,7)={result}");

    // Struct
    let p = Point::new(3.0, 4.0);
    println!("distance_sq={}", p.distance_sq());

    // Enum + match
    println!("direction={}", direction_label(Direction::North));

    // Vec and String (exercises alloc)
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
    println!("vec sum={}", v.iter().sum::<i32>());

    let s = String::from("hello from validation crate");
    println!("{s}");
}
