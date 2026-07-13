#include <stdint.h>

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
