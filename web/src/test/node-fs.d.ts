declare module "node:fs" {
  export function mkdirSync(path: URL, options: { recursive: boolean }): void;
  export function writeFileSync(path: URL, data: string): void;
}
