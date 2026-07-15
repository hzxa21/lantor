import { Camera, Shuffle } from "lucide-react";
import { useRef, useState, type ReactNode } from "react";
import { randomDylanAvatarSpec } from "../avatar-utils";

type AvatarPickerProps = {
  children: ReactNode;
  seedHint: string;
  onChange: (value: string) => void;
};

const AVATAR_IMAGE_SIZE = 256;
const MAX_AVATAR_SOURCE_BYTES = 20 * 1024 * 1024;

function loadImage(file: File) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const objectUrl = URL.createObjectURL(file);
    const image = new Image();

    image.onload = () => {
      URL.revokeObjectURL(objectUrl);
      resolve(image);
    };
    image.onerror = () => {
      URL.revokeObjectURL(objectUrl);
      reject(new Error("That image could not be opened"));
    };
    image.src = objectUrl;
  });
}

async function avatarDataUrlFromFile(file: File) {
  if (file.size > MAX_AVATAR_SOURCE_BYTES) {
    throw new Error("Choose an image smaller than 20 MB");
  }
  if (file.type && !file.type.startsWith("image/")) {
    throw new Error("Choose an image file");
  }

  const image = await loadImage(file);
  if (!image.naturalWidth || !image.naturalHeight) {
    throw new Error("That image has no readable dimensions");
  }

  const canvas = document.createElement("canvas");
  canvas.width = AVATAR_IMAGE_SIZE;
  canvas.height = AVATAR_IMAGE_SIZE;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("This browser cannot prepare the image");

  const cropSize = Math.min(image.naturalWidth, image.naturalHeight);
  const sourceX = (image.naturalWidth - cropSize) / 2;
  const sourceY = (image.naturalHeight - cropSize) / 2;
  context.drawImage(
    image,
    sourceX,
    sourceY,
    cropSize,
    cropSize,
    0,
    0,
    AVATAR_IMAGE_SIZE,
    AVATAR_IMAGE_SIZE,
  );

  const webp = canvas.toDataURL("image/webp", 0.88);
  return webp.startsWith("data:image/webp") ? webp : canvas.toDataURL("image/png");
}

export function AvatarPicker({ children, seedHint, onChange }: AvatarPickerProps) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState("");

  async function selectImage(file: File | undefined) {
    if (!file) return;
    setError("");
    setUploading(true);
    try {
      onChange(await avatarDataUrlFromFile(file));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to prepare that image");
    } finally {
      setUploading(false);
    }
  }

  return (
    <>
      <button
        type="button"
        className="avatar-image-picker"
        title="Change avatar"
        aria-label={uploading ? "Preparing avatar" : "Change avatar"}
        disabled={uploading}
        onClick={() => fileInputRef.current?.click()}
      >
        {children}
        <span className="avatar-image-picker-badge" aria-hidden="true">
          <Camera size={13} />
        </span>
      </button>
      <input
        ref={fileInputRef}
        className="file-input-hidden"
        type="file"
        accept="image/*"
        onChange={(event) => {
          void selectImage(event.target.files?.[0]);
          event.target.value = "";
        }}
      />
      <button
        type="button"
        className="agent-form-preview-avatar-action"
        title="Random avatar"
        aria-label="Random avatar"
        disabled={uploading}
        onClick={() => {
          setError("");
          onChange(randomDylanAvatarSpec(seedHint));
        }}
      >
        <Shuffle size={16} />
      </button>
      {error && <p className="modal-field-error avatar-picker-error" role="alert">{error}</p>}
    </>
  );
}
