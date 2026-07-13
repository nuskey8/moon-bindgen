///|
test "LZ4 FFI" {
  let input = b"MoonBit and LZ4! MoonBit and LZ4! MoonBit and LZ4!"
  let compressed = Bytes::make(lz4_compress_bound(input.length()), b'\x00')
  let compressed_size = lz4_compress_default(
    input,
    compressed,
    input.length(),
    compressed.length(),
  )
  assert_true(compressed_size > 0)
  assert_true(compressed_size < input.length())
  let output = Bytes::make(input.length(), b'\x00')
  let output_size = lz4_decompress_safe(
    compressed,
    output,
    compressed_size,
    output.length(),
  )
  assert_eq(output_size, input.length())
  assert_eq(output, input)
}

///|
test "LZ4 FFI with malformed input" {
  let malformed = b"not an lz4 block"
  let output = Bytes::make(128, b'\x00')
  let result = lz4_decompress_safe(
    malformed,
    output,
    malformed.length(),
    output.length(),
  )
  assert_true(result < 0)
}
