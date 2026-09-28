//! Two PNG screenshots compared pixel by pixel inside the Weles page: both
//! are decoded there, drawn at the top left of one union-sized canvas (the
//! area only one covers counts as differing, so a size mismatch is a parity
//! failure), and a pixel differs when its furthest channel moves more than
//! the tolerance. With `mask`, the differing pixels are painted red over a
//! faded candidate so a failure ships a picture of where it is wrong.

use serde_json::json;

use crate::specs::web::weles::Page;
use crate::specs::*;

const COMPARE: &str = r#"async (candidate, reference, tolerance, mask) => {
  const decode = (base64) => new Promise((resolve, reject) => {
    const image = new Image();
    image.addEventListener("load", () => resolve(image));
    image.addEventListener("error", () => reject(new Error("a parity screenshot did not decode")));
    image.src = `data:image/png;base64,${base64}`;
  });
  const [left, right] = await Promise.all([decode(candidate), decode(reference)]);
  const width = Math.max(left.naturalWidth, right.naturalWidth);
  const height = Math.max(left.naturalHeight, right.naturalHeight);
  const surface = (image) => {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(image, 0, 0);
    return context.getImageData(0, 0, width, height).data;
  };
  const a = surface(left);
  const b = surface(right);
  const overlay = mask ? new ImageData(width, height) : null;
  let differing = 0;
  for (let index = 0; index < a.length; index += 4) {
    const delta = Math.max(
      Math.abs(a[index] - b[index]),
      Math.abs(a[index + 1] - b[index + 1]),
      Math.abs(a[index + 2] - b[index + 2]),
      Math.abs(a[index + 3] - b[index + 3]),
    );
    const differs = delta > tolerance;
    if (differs) differing += 1;
    if (!overlay) continue;
    const pixel = differs ? [217, 45, 45] : [0, 1, 2].map((channel) => 255 - ((255 - a[index + channel]) >> 3));
    overlay.data.set([...pixel, 255], index);
  }
  let maskData = null;
  if (overlay) {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").putImageData(overlay, 0, 0);
    maskData = canvas.toDataURL("image/png").split(",")[1];
  }
  return {
    width,
    height,
    differing,
    total: width * height,
    ratio: differing / (width * height),
    candidate: { width: left.naturalWidth, height: left.naturalHeight },
    reference: { width: right.naturalWidth, height: right.naturalHeight },
    mask: maskData,
  };
}"#;

/// Compare the two PNG files in `page`; the answer is COMPARE's result.
pub(super) fn compare(
    page: &Page,
    candidate: &Path,
    reference: &Path,
    tolerance: u64,
    mask: bool,
) -> Result<Value, String> {
    let encode = |path: &Path| {
        fs::read(path)
            .map(|bytes| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes))
            .map_err(|error| format!("{}: {error}", path.display()))
    };
    let arguments = json!([encode(candidate)?, encode(reference)?, tolerance, mask]);
    page.evaluate(&format!("({COMPARE})(...{arguments})"))
}
