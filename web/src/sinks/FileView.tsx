import type { PresentedFile } from "../lib/types";

export function FileView({ file }: { file: PresentedFile }) {
  return (
    <div className="flex flex-col items-start gap-2">
      {file.image && (
        <img src={file.url} alt={file.name} className="max-h-96 max-w-full object-contain" />
      )}
      <a href={file.url} download={file.name} className="text-accent underline">
        Download {file.name}
      </a>
    </div>
  );
}
