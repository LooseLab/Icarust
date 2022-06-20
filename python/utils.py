from pathlib import Path
from pathlib import Path

import pandas as pd
import numpy as np
from numpy.typing import NDArray

def collapse_amplicon_start_ends(scheme_bed_file: Path) -> NDArray:
    """
    Collapse a primer scheme bed file into start stop coordinates that we can use to slice the reference

    Parameters
    ----------
    scheme_bed_file: Path
        The scheme bed file

    Returns
    -------
    NDArray
        2D Array of start stop pairs for amplicons and teh name of the amplicon


    """
    df = pd.read_csv(scheme_bed_file, sep="\t", header=None, names=["chromosome", "start", "end", "name",], usecols=[0,1,2,3])
    df = df[df.columns[~df.isnull().all()]]
    df["primer_number"] = pd.to_numeric(df["name"].str.split("_").str[2])
    df = df.set_index("primer_number")
    df[["primer_start", "primer_end"]] = df.groupby("primer_number").agg(
        {"start": np.min, "end": np.max}
    )
    df = df.reset_index()
    df = df.set_index(["primer_start", "primer_end"])
    df = df.loc[~df.index.duplicated(keep="first")]
    df = df.reset_index()
    amplicon_coords = np.column_stack(
        (
            df["primer_start"].values,
            df["primer_end"].values,
            df["name"].values
        )
    )
    return amplicon_coords
