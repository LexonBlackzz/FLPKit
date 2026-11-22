"""Install folders the converters copy samples and read presets from. Set FLPKIT_IL to your Image-Line folder
and FLPKIT_PF to the folder holding FLStudio4 / FruityLoops3.5 if they're not in the installers' default place."""
import os, glob

PF = os.environ.get('FLPKIT_PF', 'C:/Program Files (x86)').rstrip('/\\') + '/'
IL = os.environ.get('FLPKIT_IL', PF + 'Image-Line').rstrip('/\\') + '/'
NEWEST = max(glob.glob(IL + 'FL Studio 202*'), default=IL + 'FL Studio 2026.1.1')  # FL 2025 / 2026 install
