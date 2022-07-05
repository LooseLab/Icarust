from ast import Str
from faulthandler import disable
import json
import logging
from typing import Dict, Tuple
import argparse
import time

from mappy import fastx_read
import numpy as np
from numpy.typing import NDArray
from pathlib import Path
from rich.console import Console
from scrappy import sequence_to_squiggle
from rich.logging import RichHandler
from rich.live import Live
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, BarColumn, TextColumn, TaskID
from rich.table import Table
from scipy.stats import skewnorm

from utils import append_barcode_sequence, collapse_amplicon_start_ends, create_dir, get_barcode_seq

logger = logging.getLogger(__name__)
logger.addHandler(RichHandler())
logger.setLevel(logging.INFO)
console = Console()

# todo move to utils
def progress_bar_setup(total_files: int) -> Tuple[Progress, Progress, Table, Dict[Str, TaskID]]:
    """
    Setup the Live env for the rich progress bar
    Parameters
    ---------
    total_files: int
        The total number of reference sequences to squiggleify
    Returns
    -------
    A tuple of 2 progress bars and the Table they are embedded in
    """
    job_progress = Progress(
        "{task.description}",
        SpinnerColumn(),
        BarColumn(),
        TextColumn("[progress.percentage]{task.percentage:>3.0f}%"),
    )
    contig_job = job_progress.add_task("[green]Contigs")
    chunk_job = job_progress.add_task("[magenta]Chunks", total=0)
    amplicon_job = job_progress.add_task("[cyan]Amplicons", total=0)

    overall_progress = Progress()
    overall_task = overall_progress.add_task("All Jobs", total=total_files)

    progress_table = Table.grid()
    job_dict = {"contig_job": contig_job,
        "chunk_job": chunk_job,
        "amplicon_job": amplicon_job,
        "overall_job": overall_task
    }
    progress_table.add_row(
        Panel.fit(
            overall_progress, title="Overall Progress", border_style="green", padding=(2, 2)
        ),
        Panel.fit(job_progress, title="[b]Jobs", border_style="red", padding=(1, 2)),
    )
    return job_progress, overall_progress, progress_table, job_dict

def get_sequence(path: Path, out_dir: Path, job_progress: Progress, task_lookup: Dict[str, int], skew: int, barcode: Str, bed_file: Path = None) -> None:
    """
    Use pyfastx to open a file and feed the sequences in turn to generate_squiggle
    Parameters
    ----------
    path: Path
        Path to reference file
    out_dir: Path
        Path to the output directory to write squiggle into, if not provided by user default is None
    job_progress: Progress
        Progress class for the internal for loops
    task_lookup: Dict
        Look_up the task ids to update ther progress in Rich
    skew: int
        Degree of skew for a normal distribution. 0 if contig lengths are to be used
    barcode: Str
        The barcode for this reference,
    bed_file: Path
        Bed file containing amplicons, if one is provided, default None
    Returns
    -------

    """
    json_file_path = out_dir / "distributions.json"
    assert path.exists(), "Can't FinD THiS reference FilE"
    seq_lens = []
    total_contigs = sum((1 for name, seq, qual in fastx_read(str(path.resolve()))))
    job_progress.update(task_lookup["contig_job"], total=total_contigs)
    for name, seq, qual in fastx_read(str(path.resolve())):
        logger.info(f"Generating squiggle for {name}")
        if args.bed_file is not None:
            logger.info("Bedfile found - parsing and splitting sequence into amplicons...")
            coords = collapse_amplicon_start_ends(bed_file)
            job_progress.update(task_lookup["amplicon_job"], total=len(coords))
            for amp_start, amp_stop, amp_name in coords:
                amp_seq = seq[amp_start:amp_stop]
                seq_lens.append((amp_name, len(amp_seq)))
                if barcode is not None:
                    barcode_1_seq, barcode_2_seq = get_barcode_seq(barcode)
                amp_seq = append_barcode_sequence(amp_seq, barcode_seq_1=barcode_1_seq, barcode_seq_2=barcode_2_seq)
                logger.info(f"Amplicon {amp_name} spans reference from {amp_start}: {amp_stop}")
                squiggle_path = out_dir / f"{name}_{amp_name}.squiggle.npy"
                if not squiggle_path.exists():
                    generate_squiggle(amp_seq, squiggle_path, job_progress, task_lookup)
                job_progress.advance(task_lookup["amplicon_job"])
        else:
            squiggle_path = out_dir / f"{name}.squiggle.npy"
            seq_lens.append((str(name), len(seq)))
            if not squiggle_path.exists():
                generate_squiggle(seq, squiggle_path, job_progress, task_lookup)
            else:
                logger.warning(f"File with name {squiggle_path} already exists. Skipping...")
        job_progress.advance(task_lookup["contig_job"])
    logger.info("Creating distributions file.")
    if skew == 0:
        distributions = generate_distribution(sorted(seq_lens, key=lambda x: x[0]))
    else:
        distributions = (np.abs(skewnorm.rvs(args.skew, scale=7, size=len(seq_lens)).round().astype(int)).tolist(), [name for name, _ in seq_lens])
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

def generate_squiggle(seq: Str, squiggle_path: Path, job_progress: Progress, task_lookup: Dict[str, int]) -> None:
    """
    Generate squiggle from a given Fasta Sequence
    Parameters
    ----------
    seq: str
        Sequence object as provided by mappy fastx_read

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
    task_id = task_lookup["chunk_job"]
    job_progress.update(task_id, total=np.ceil(seq_len/100_000))
    for i in range(0, seq_len, chunk_size):
        seq_segment = str(seq)[i: i+chunk_size]
        squiggle = sequence_to_squiggle(
            str(seq_segment).strip(), rescale=True).data(as_numpy=True, sloika=False)
        new_squiggle = expandy(squiggle, n)
        new_squiggle = (((new_squiggle * stdev) + mean) * digitisation) / range_scale
        new_squiggle = new_squiggle.astype(np.int16)
        arr = np.concatenate([arr[:], new_squiggle])[:]
        job_progress.advance(task_id)
    np.save(squiggle_path, arr=arr)

def validate_args (args: argparse.Namespace) -> None:
    """
    Validate the provided args
    Parameters
    ----------
    args: argparse.Namespace
        Arguments provided on the command line
    Returns
    -------
    None
    """
    # If we have more than one reference file and we have provided a BED file
    if len(args.reference_files) > 1 and args.bed_file is not None:
        raise Exception("If Bed file is provided, please only specify one reference")
    
    if args.bed_file and not args.bed_file.exists():
        raise FileNotFoundError(f"{args.bed_file} not found. Please double check provided path...")

    if not args.out_dir.exists():
        logger.warning(f"{args.out_dir} does not exist, attempting to create it")
        create_dir(args.out_dir)
    
    if args.barcode is not None:
        assert len(args.barcode) == len(args.reference_files), "Provided number of barcodes must equal number of reference files."
        for barcode in args.barcode:
            print(barcode)
            barcode_file_path = Path(f"python/barcoding/fasta/{barcode}.fasta").resolve()
            assert barcode_file_path.exists(), "%s does not exist. Looking for the barcoding directory in the base icarust source code directory." % barcode_file_path
    logger.info("Validations passed ✨")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Create squiggle from a reference file.')
    parser.add_argument('reference_files', metavar='files', type=Path, nargs='+',
                    help='Space seperated reference fastas to convert to squiggle.')
    parser.add_argument("--bed_file", type=Path, help="Bed file for primer schemes. Splits reference into amplicons, producing a squiggle file for each amplicon. Only works with individual reference files.", default=None)
    parser.add_argument("--out_dir", type=Path, default=Path.cwd(), help="Directory to write out the squiggle arrays to. Defaults to current working directory.")
    parser.add_argument("--rev", action="store_true", help="Produce reverse and forward squiggle files, default False")
    parser.add_argument("--skew", type=int, help="An int representing the degree of skew to be applied to the relative rate that we see different contigs. Default 0 - for an even genome length based skew", default=0)
    parser.add_argument("--barcode", type=str, help="Barcode to include. Must be between one and twelve, in the format Barcode01 Barcode02 etc. If provided, Number of barcodes must equal number of reference files.", nargs="+", default=None)
    args = parser.parse_args()

    validate_args(args)
    job_progress, overall_progress, progress_table, task_lookup = progress_bar_setup(len(args.reference_files))
    # If we don't have barcodes, create a list of none to zip them together.
    barcodes = args.barcode if args.barcode is not None else [None] * len(args.reference_files)
    reference_files = zip(args.reference_files, barcodes)
    completed_overall = 1
    with Live(progress_table, refresh_per_second=10):
        for ref_filepath, barcode in reference_files:
            get_sequence(ref_filepath.resolve(), args.out_dir, job_progress, task_lookup, args.skew, barcode, args.bed_file)
            overall_progress.update(task_lookup["overall_job"], completed=completed_overall)
            completed_overall += 1

