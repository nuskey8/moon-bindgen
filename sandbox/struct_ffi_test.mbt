///|
test "struct values through generated C stub" {
  let point = TestPoint::{
    x: 10,
    y: 20,
    rgba: [b'\x01', b'\x02', b'\x03', b'\x04'],
  }
  let moved = test_point_translate(point, 7, -5)
  assert_eq(moved.x, 17)
  assert_eq(moved.y, 15)
  assert_eq(moved.rgba[0], b'\x01')
  assert_eq(moved.rgba[3], b'\x04')
  assert_eq(test_point_score(moved), 37)
}

///|
test "nested struct values through generated C stub" {
  let color : FixedArray[Byte] = [b'\x01', b'\x02', b'\x03', b'\x04']
  let line = TestLine::{
    start: TestPoint::{ x: 1, y: 2, rgba: color.copy() },
    end: TestPoint::{ x: 10, y: 20, rgba: color.copy() },
  }
  let moved = test_line_translate(line, 3, 4)
  assert_eq(moved.start.x, 4)
  assert_eq(moved.start.y, 6)
  assert_eq(moved.end.x, 13)
  assert_eq(moved.end.y, 24)
  assert_eq(moved.end.rgba[3], b'\x04')
}

///|
test "pointer field in an input struct" {
  let data = b"\x01\x02\x03\x04"
  let slice = TestSlice::{ data, len: data.length().to_uint64() }
  assert_eq(test_slice_sum(slice), 10U)
}
