///|
fn byte_pointer_outputs() -> (
  FixedArray[@c.ReadOnlyPointer[Byte]],
  UInt64,
) {
  let pointers : FixedArray[@c.ReadOnlyPointer[Byte]] = [
    @c.ReadOnlyPointer::null(),
  ]
  let lengths : FixedArray[@c.CSize] = [0UL]
  let length_pointer = @c.CSizeArrayAsPointer::as_pointer(lengths)
  test_byte_pointer(
    @c.ReadOnlyPointerArray::as_pointer(pointers),
    length_pointer,
  )
  (pointers, length_pointer.as_readonly().read_size())
}

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
  let (pointers, length) = byte_pointer_outputs()
  let slice = TestSlice::{
    data: pointers[0],
    len: length,
  }
  assert_eq(test_slice_sum(slice), 10U)
}

///|
test "external pointer retains null" {
  assert_true(test_context_null().is_null())
}

///|
test "double pointer to an external context" {
  let context = test_context_ptr()
  test_context_create(context)
  assert_eq(
    test_context_value(context.as_readonly().read_pointer().as_readonly()),
    1234,
  )
}

///|
test "triple pointer updates the deepest external context" {
  let context = test_context_ptr_ptr()
  test_context_pointer_pointer(context)
  let inner = context.as_readonly().read_pointer().as_readonly().read_pointer()
  assert_eq(test_context_value(inner.as_readonly()), 1234)
}

///|
test "double pointer to a primitive" {
  let values : FixedArray[@c.Pointer[Int]] = [@c.Pointer::null()]
  test_int_pointer(@c.PointerArray::as_pointer(values))
  let value = values[0]
  assert_eq(value.as_readonly().read_int32(), 42)
  value.write_int32(99)
  assert_eq(value.as_readonly().read_int32(), 99)
}

///|
test "double pointer to borrowed bytes" {
  let (pointers, length) = byte_pointer_outputs()
  assert_eq(length, 4UL)
  assert_eq(pointers[0].read_uint8(), 1)
}
