#include <stdint.h>
#include <stddef.h>

typedef struct TestPoint {
  int32_t x;
  int32_t y;
  uint8_t rgba[4];
} TestPoint;

typedef struct TestLine {
  TestPoint start;
  TestPoint end;
} TestLine;

typedef struct TestSlice {
  const uint8_t *data;
  uint64_t len;
} TestSlice;

typedef struct TestContext {
  int32_t value;
} TestContext;

static TestContext test_context = {1234};
static TestContext *test_context_slot;
static TestContext **test_context_slot_pointer = &test_context_slot;
static int32_t test_integer = 42;
static const uint8_t test_bytes[] = {1, 2, 3, 4};

TestPoint test_point_translate(TestPoint point, int32_t dx, int32_t dy) {
  point.x += dx;
  point.y += dy;
  return point;
}

int32_t test_point_score(TestPoint point) {
  return point.x + point.y + point.rgba[0] + point.rgba[3];
}

TestLine test_line_translate(TestLine line, int32_t dx, int32_t dy) {
  line.start = test_point_translate(line.start, dx, dy);
  line.end = test_point_translate(line.end, dx, dy);
  return line;
}

uint32_t test_slice_sum(TestSlice slice) {
  uint32_t result = 0;
  for (uint64_t i = 0; i < slice.len; i++) {
    result += slice.data[i];
  }
  return result;
}

TestContext *test_context_null(void) {
  return 0;
}

TestContext **test_context_ptr(void) {
  return &test_context_slot;
}

TestContext ***test_context_ptr_ptr(void) {
  return &test_context_slot_pointer;
}

void test_context_create(TestContext **out) {
  *out = &test_context;
}

int32_t test_context_value(const TestContext *context) {
  return context->value;
}

void test_context_pointer_pointer(TestContext ***out) {
  **out = &test_context;
}

void test_int_pointer(int32_t **out) {
  *out = &test_integer;
}

void test_byte_pointer(const uint8_t **out, size_t *out_len) {
  *out = test_bytes;
  *out_len = sizeof(test_bytes);
}
