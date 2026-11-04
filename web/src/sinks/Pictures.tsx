import { capturedImageUrl } from "../lib/api";
import type { CapturedImage } from "../lib/types";

export function Pictures({
  pictures,
  limit,
}: {
  pictures: readonly CapturedImage[];
  limit: number;
}) {
  return (
    <div className={`grid gap-1 p-3 ${limit === 1 ? "grid-cols-1" : "grid-cols-2"}`}>
      {pictures.slice(0, limit).map((picture) => (
        <Picture key={picture.id} picture={picture} />
      ))}
    </div>
  );
}

function Picture({ picture }: { picture: CapturedImage }) {
  const title = `${picture.mode} ${picture.width} × ${picture.height}`;
  if (picture.image == null) {
    return (
      <span className="text-2xs text-warn" title={picture.image_error ?? title}>
        {picture.mode}
      </span>
    );
  }
  const url = capturedImageUrl(picture.image.url);
  return (
    <a href={url} target="_blank" rel="noreferrer" title={title}>
      <img src={url} alt={title} className="w-full rounded-inset object-contain" />
    </a>
  );
}
