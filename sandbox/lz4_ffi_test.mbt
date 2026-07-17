///|
test "LZ4 FFI" {
  let input = b"MoonBit and LZ4! MoonBit and LZ4! MoonBit and LZ4!"
  let input_length = input.length()
  let compressed_capacity = lz4_compress_bound(input_length)
  let compressed : FixedArray[Byte] = FixedArray::make(
    compressed_capacity,
    b'\x00',
  )
  let compressed_size = @c.ByteArrayAsPointer::with_pointer(
    compressed,
    fn(destination) {
      lz4_compress_default(
        @c.BytesAsPointer::as_pointer(input).unsafe_cast(),
        destination.unsafe_cast(),
        input_length,
        compressed_capacity,
      )
    },
  )
  assert_true(compressed_size > 0)
  assert_true(compressed_size < input_length)
  let output : FixedArray[Byte] = FixedArray::make(input_length, b'\x00')
  let output_size = @c.ByteArrayAsPointer::with_pointer(
    compressed,
    fn(source) {
      @c.ByteArrayAsPointer::with_pointer(
        output,
        fn(destination) {
          lz4_decompress_safe(
            source.as_readonly().unsafe_cast(),
            destination.unsafe_cast(),
            compressed_size,
            input_length,
          )
        },
      )
    },
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
  let result = @c.ByteArrayAsPointer::with_pointer(
    output,
    fn(destination) {
      lz4_decompress_safe(
        @c.BytesAsPointer::as_pointer(malformed).unsafe_cast(),
        destination.unsafe_cast(),
        malformed_length,
        128,
      )
    },
  )
  assert_true(result < 0)
}
