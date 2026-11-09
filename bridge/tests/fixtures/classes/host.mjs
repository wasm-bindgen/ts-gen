export class Point {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }
  static origin() {
    return new Point(0, 0);
  }
  norm() {
    return Math.hypot(this.x, this.y);
  }
}
class Hidden {
  get secret() {
    return "s3";
  }
}
export function makeHidden() {
  return new Hidden();
}
