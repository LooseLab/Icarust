from faulthandler import disable
import json
import logging
from typing import Tuple
import argparse

import pyfastx as pyfastx
import numpy as np
from numpy.typing import NDArray
from pathlib import Path
from rich.console import Console
from scrappy import sequence_to_squiggle
from rich.logging import RichHandler
from tqdm.rich import trange, tqdm

logger = logging.getLogger(__name__)
logger.addHandler(RichHandler())
logger.setLevel(logging.INFO)
console = Console()

def get_sequence(path: Path) -> None:
    """
    Use pyfastx to open a file and feed the sequences in turn to generate_squiggle
    Parameters
    ----------
    path: Path
        Path to reference file
    Returns
    -------

    """
    json_file_path = Path(f"../distributions.json")
    assert path.exists(), "Can't FinD THiS FilE"
    fa = pyfastx.Fasta(str(path))
    seq_lens = []
    for seq in fa:
        squiggle_path = Path(f"../{seq.name}.squiggle.npy")
        seq_lens.append((str(seq.name), len(seq)))
        if not squiggle_path.exists():
            generate_squiggle(seq)
        else:
            logger.warning(f"File with name {squiggle_path} already exists. Skipping...")
    logger.info("Creating distributions file.")
    distributions = generate_distribution(sorted(seq_lens, key=lambda x: x[0]))
    write_distribution_json(json_file_path, distributions)


def append_distributions(file_path: Path, distributions: Tuple[Tuple[int], Tuple[str]]) -> dict[str, list[int]]:
    """
    Append outr new distirbutions with the existing distributions already in the file.
    """ 
    with open(file_path, "r") as fh:
        disty = json.load(fh)
        disty["weights"].extend(list(distributions[0]))
        disty["names"].extend(list(distributions[1]))
    return disty

def write_distribution_json(file_path: Path, distributions: Tuple[Tuple[int], Tuple[str]]) -> None:
    """
    Write our distributions out
    Parameters
    ----------
    file_path: Path
        Path to write the JSON file to
    distributions: Tuple of Tuples
        Distributions to write into file

    Returns
    -------
    None

    """
    console.log(distributions)
    if file_path.exists():
        json_dict = append_distributions(file_path, distributions)
    else:
        json_dict = {
            "weights": list(distributions[0]),
            "names": list(distributions[1])
        }
    
    with open(file_path, "w") as fh:
        json.dump(obj=json_dict, fp=fh)


def generate_distribution(contig_lengths: list[Tuple[str, int]]) -> Tuple[Tuple[int], Tuple[str]]:
    """

    Parameters
    ----------
    contig_lengths: list
        The contig lengths, to weight a distribution

    Returns
    -------
    tuple of tuples
        Tuples of Cumulative weights and order of genomes

    """
    contig_lens, contig_names = [], []
    console.log(contig_lengths)
    for contig_name, contig_len in contig_lengths:
        contig_lens.append(contig_len)
        contig_names.append(contig_name)
    return tuple(contig_lens), tuple(contig_names)


def expandy(arr: NDArray, n: int) -> NDArray:
    """
    This is a super slow function that we need to run in some other way to make it more efficient.
    :param arr:
    :param n:
    :return:
    """
    return np.random.laplace(np.repeat(arr[:, 0], arr[:, 2].astype(int)), np.repeat(arr[:, 1]*n, arr[:, 2].astype(int)))


def generate_squiggle(seq: pyfastx.Sequence) -> None:
    """
    Generate squiggle from a given Fasta Sequence
    Parameters
    ----------
    seq: pyfastx.Sequence
        Sequence object as provided by pyfastx

    Returns
    -------
    None
    """
    range_scale = 1350
    digitisation = 8192
    mean = 80
    stdev = 13
    seq_len = len(seq)
    chunk_size = 100_000
    n = 1 / np.sqrt(2)
    arr = np.array([], dtype=np.int16)

    for i in trange(0, seq_len, chunk_size):
        seq_segment = seq[i: i+chunk_size]
        squiggle = sequence_to_squiggle(
            str(seq_segment), rescale=True).data(as_numpy=True, sloika=False)
        new_squiggle = expandy(squiggle, n)
        new_squiggle = (((new_squiggle * stdev) + mean) * digitisation) / range_scale
        new_squiggle = new_squiggle.astype(np.uint8)
        arr = np.concatenate([arr[:], new_squiggle])[:]
    np.save(f"../{seq.name}.squiggle", arr=arr)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Process some integers.')
    parser.add_argument('reference_files', metavar='files', type=Path, nargs='+',
                    help='Space seperated reference fastas to convert to squiggle.')
    args = parser.parse_args()
    for filepath in args.reference_files:
        get_sequence(filepath.resolve())

