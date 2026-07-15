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

///|
test "external pointer return is nullable by default" {
  match test_context_null() {
    None => ()
    Some(_) => fail("expected None")
  }
}

///|
test "double pointer to an external context" {
  let context = Ref(moon_bindgen_ptr_testcontext_null())
  test_context_create(context)
  assert_eq(test_context_value(moon_bindgen_ptr_testcontext_deref(context.val)), 1234)
}

///|
test "triple pointer updates the deepest external context" {
  let context = Ref(test_context_ptr())
  test_context_pointer_pointer(context)
  let inner = moon_bindgen_ptr_ptr_testcontext_deref(context.val)
  assert_eq(test_context_value(moon_bindgen_ptr_testcontext_deref(inner)), 1234)
}

///|
test "double pointer to a primitive" {
  let value = Ref(moon_bindgen_ptr_int_null())
  test_int_pointer(value)
  assert_eq(moon_bindgen_ptr_int_read(value.val), 42)
}

///|
test "double pointer to borrowed bytes" {
  let pointer = Ref(moon_bindgen_constptr_byte_null())
  let length = Ref(0UL)
  test_byte_pointer(pointer, length)
  assert_eq(length.val, 4UL)
  assert_eq(
    moon_bindgen_constptr_byte_copy_bytes(pointer.val, length.val),
    b"\x01\x02\x03\x04",
  )
}
