#[repr(C)]
pub struct TestPoint {
    pub x: i32,
    pub y: i32,
    pub rgba: [u8; 4],
}

#[repr(C)]
pub struct TestLine {
    pub start: TestPoint,
    pub end: TestPoint,
}

#[repr(C)]
pub struct TestSlice {
    pub data: *const u8,
    pub len: usize,
}

unsafe extern "C" {
    pub fn test_point_translate(point: TestPoint, dx: i32, dy: i32) -> TestPoint;
    pub fn test_point_score(point: TestPoint) -> i32;
    pub fn test_line_translate(line: TestLine, dx: i32, dy: i32) -> TestLine;
    pub fn test_slice_sum(slice: TestSlice) -> u32;
}
