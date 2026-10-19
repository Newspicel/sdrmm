export class RowQueue {
  readonly bins: number;
  private readonly capacity: number;
  private readonly rows: Uint8Array;
  private head = 0;
  private length = 0;

  constructor(bins: number, capacity: number) {
    this.bins = Math.max(1, bins);
    this.capacity = Math.max(1, capacity);
    this.rows = new Uint8Array(this.capacity * this.bins);
  }

  get pending(): number {
    return this.length;
  }

  push(row: Uint8Array): void {
    const at = (this.head + this.length) % this.capacity;
    this.rows.set(row.subarray(0, this.bins), at * this.bins);
    if (this.length === this.capacity) {
      this.head = (this.head + 1) % this.capacity;
    } else {
      this.length += 1;
    }
  }

  drain(emit: (rows: Uint8Array, count: number) => void): void {
    while (this.length > 0) {
      const span = Math.min(this.length, this.capacity - this.head);
      emit(this.rows.subarray(this.head * this.bins, (this.head + span) * this.bins), span);
      this.head = (this.head + span) % this.capacity;
      this.length -= span;
    }
  }

  clear(): void {
    this.head = 0;
    this.length = 0;
  }
}
