"""fl8to4.py in.flp out.flp: FL 8.0 -> FL 6.0 (fl8to6) -> FL 5.0 (fl6to5 + fl8to5's substitutes) -> FL 4.5.2
(fl5to4)."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths, fl8to6, fl6to5, fl5to4, fl8to5  # fl8to5 registers its FL 5 plugin substitutes in fl6to5.SUBST
from fl8to6 import read, write

fl8to6.FL6_DATA = paths.PF + 'FLStudio4/Data'  # copy every sample FL 4's Data lacks

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, fl5to4.convert(fl6to5.convert(fl8to6.convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples'))))
