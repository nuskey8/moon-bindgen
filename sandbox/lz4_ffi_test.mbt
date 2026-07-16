///|
test "LZ4 FFI" {
  let input = b"MoonBit and LZ4! MoonBit and LZ4! MoonBit and LZ4!"
  let input_length = input.length()
  let compressed_capacity = lz4_compress_bound(input_length)
  let compressed : FixedArray[Byte] = FixedArray::make(
    compressed_capacity,
    b'\x00',
  )
  let compressed_size = lz4_compress_default(
    @c.BytesAsPointer::as_pointer(input).unsafe_cast(),
    @c.ByteArrayAsPointer::as_pointer(compressed).unsafe_cast(),
    input_length,
    compressed_capacity,
  )
  assert_true(compressed_size > 0)
  assert_true(compressed_size < input_length)
  let output : FixedArray[Byte] = FixedArray::make(input_length, b'\x00')
  let output_size = lz4_decompress_safe(
    @c.ByteArrayAsPointer::as_pointer(compressed).as_readonly().unsafe_cast(),
    @c.ByteArrayAsPointer::as_pointer(output).unsafe_cast(),
    compressed_size,
    input_length,
  )
  assert_eq(output_size, input_length)
  for i = 0; i < input_length; i = i + 1 {
    assert_eq(output[i], input[i])
  }
}

///|
test "LZ4 FFI with malformed input" {
  let malformed = b"not an lz4 block"
  let malformed_length = malformed.length()
  let output : FixedArray[Byte] = FixedArray::make(128, b'\x00')
  let result = lz4_decompress_safe(
    @c.BytesAsPointer::as_pointer(malformed).unsafe_cast(),
    @c.ByteArrayAsPointer::as_pointer(output).unsafe_cast(),
    malformed_length,
    128,
  )
  assert_true(result < 0)
}
