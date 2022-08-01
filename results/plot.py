from dataclasses import dataclass
from collections import defaultdict
from typing import List, Tuple, Dict, Optional
import os

import numpy as np
import pandas as pd
from matplotlib import pyplot as plt
import seaborn as sns


@dataclass
class Client:
    numb: int
    start_times: np.ndarray
    end_times: np.ndarray

    def durations(self) -> np.ndarray:
        return self.end_times - self.start_times


def client_from(lines: List[str]) -> Client:
    numb = int(''.join(ch for ch in lines[0] if ch.isdigit()))
    start_times = np.fromstring(lines[1], dtype=float, sep=" ")
    end_times = np.fromstring(lines[2], dtype=float, sep=" ")
    return Client(numb, start_times, end_times)


@dataclass
class Node:
    name: str
    clients: List[Client]

    def durations(self) -> np.ndarray:
        res = [client.durations() for client in self.clients]
        return np.hstack(res)


def node_from(path: str, name: str) -> Node:
    with open(path) as file:
        lines = file.readlines()
    clients = []
    for i in range(0, len(lines), 4):
        clients.append(client_from(lines[i:i+3]))
    return Node(name, clients)


# TODO: switch Node and Run <22-07-22, dvdsk noreply@davidsk.dev>
@dataclass
class Run:
    run: int
    nodes: List[Node]

    def durations(self) -> np.ndarray:
        res = [node.durations() for node in self.nodes]
        return np.hstack(res)


def run_from(dir: str, run: int, nodes: List[str]) -> Run:
    runs = []
    for node in nodes:
        path = f"{dir}/{node}_{run}.csv"
        runs.append(node_from(path, node))
    return Run(run, runs)


@dataclass
class Data:
    runs: List[Run]

    def durations(self) -> np.ndarray:
        res = [node.durations() for node in self.runs]
        return np.hstack(res)

    def flatten(self) -> Tuple[np.ndarray, np.ndarray]:
        start_times = []
        end_times = []
        for run in self.runs:
            for node in run.nodes:
                for client in node.clients:
                    start_times.append(client.start_times)
                    end_times.append(client.end_times)
        return np.hstack(start_times), np.hstack(end_times)


def data_from(dir: str) -> Data:
    nodes_by_run = defaultdict(list)
    for fname in os.listdir(dir):
        node = fname.split("_")[0]
        run = int(fname.split("_")[1].split(".")[0])
        nodes_by_run[run].append(node)

    runs = []
    for run, nodes in nodes_by_run.items():
        runs.append(run_from(dir, run, nodes))
    return Data(runs)


def read_and_writes():
    times = np.loadtxt("data/RangeByRow/1000_1/node117.csv", delimiter=",")
    print(times)

# see: https://stackoverflow.com/questions/46245035/pandas-dataframe-remove-outliers


def remove_outliers(df, col, n_std):
    mean = df[col].mean()
    sd = df[col].std()

    df = df[(df[col] <= mean+(n_std*sd))]

    return df


def Remove_Outlier_Indices(df):
    Q1 = df.quantile(0.25)
    Q3 = df.quantile(0.75)
    IQR = Q3 - Q1
    trueList = ~((df < (Q1 - 1.5 * IQR)) | (df > (Q3 + 1.5 * IQR)))
    return trueList


def catargory_dur(catagories: Dict[str, str], hue_title: Optional[str],
                  operation: str, x_label: str, x_values: Dict[str, str],
                  violinplot: Optional[Dict] = None, y_mul: float = 1):
    data = {
        x_label: list(),
        operation: list(),
    }
    if hue_title is not None:
        data[hue_title] = list()

    print(data)

    for (catarory, dir) in catagories.items():
        for (subdir, label) in x_values.items():
            durations = data_from(f"data/{dir}/{subdir}").durations() * y_mul
            data[operation].append(durations)
            data[x_label].append(np.full(durations.size, label))
            if hue_title is not None:
                data[hue_title].append(np.full(durations.size, catarory))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)
    filterd = remove_outliers(data, operation, 2)

    sns.set_style("whitegrid")
    plt.plot()
    sns.despine(bottom=True)
    # plt.yscale("log") # do log when keeping outliers
    if violinplot is None:
        sns.boxplot(x=x_label, y=operation,
                    hue=hue_title, data=filterd, showfliers=False)
    else:
        sns.violinplot(x=x_label, y=operation,
                       hue=hue_title, data=filterd, cut=0, **violinplot)

    # sns.swarmplot(x=x_label, y=operation,
    #               hue=hue_title, data=filterd)


def dur_dist(bench: str, operation: str, ylabel: str, xlabel: str,
             xlim: Optional[Tuple[float, float]] = None, histplot: bool = False):
    # headers: n_ministeries, duration, access_pattern
    x_mul = 1000
    data = {
        "number of ministries": [],
        f"{operation} duration": [],
    }
    print(data)

    for n in [1, 2, 3, 4, 5]:
        durations = data_from(f"data/{bench}/{n}").durations() * x_mul
        data[f"{operation} duration"].append(durations)
        data["number of ministries"].append(np.full(durations.size, n))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)
    filterd = remove_outliers(data, f"{operation} duration", 4)

    plt.plot()
    sns.despine()

    if xlim is not None:
        plt.xlim(tuple([x_mul*lim for lim in xlim]))

    if histplot:
        sns.histplot(
            data=filterd,
            x=f"{operation} duration", hue="number of ministries",
            multiple="stack",
            palette=sns.color_palette("Set2", n_colors=5),
            edgecolor=".3",
            linewidth=.1,
        )
    else:
        sns.ecdfplot(
            data=filterd,
            x=f"{operation} duration", hue="number of ministries",
            # multiple="stack",
            palette=sns.color_palette("Set2", n_colors=5),
        )
    plt.ylabel(ylabel)
    plt.xlabel(xlabel)


def dur_vs_time(bench: str):
    data = {
        "start_time": [],
        "duration": [],
        "number of ministries": []
    }

    for n in [1, 2, 3, 4, 5]:
        start_times, end_times = data_from(f"data/{bench}/{n}").flatten()
        durations = end_times - start_times
        assert(start_times.size == durations.size)
        data["start_time"].append(start_times)
        data["duration"].append(durations)
        data["number of ministries"].append(np.full(start_times.size, n))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)

    sns.set_style("whitegrid")
    plt.plot()
    sns.despine(bottom=True)

    plt.xscale("log")
    sns.scatterplot(data=data, x="start_time", y="duration",
                    hue="number of ministries", alpha=0.5)
    plt.ylabel("request completed in (seconds)")
    plt.xlabel("request started after (seconds)")


os.makedirs("plots", exist_ok=True)

# catargory_dur({"write by row": "RangeByRow", "write entire file": "RangeWholeFile"},
#               "write pattern", "write duration (ms)", "number of writers",
#               {"1000_1_1": "1", "1000_2_1": "2", "1000_4_1": "4",
#                "1000_4_2": "8", "1000_4_4": "16", "1000_4_8": "32"},
#               violinplot={"bw": 0.01, "linewidth": 0.3, "inner": None,
#                           "scale": "width", "scale_hue": False}, y_mul=1000)
# plt.savefig("plots/range_vs_writers_both.png")
# plt.clf()

# catargory_dur({"write by row": "RangeByRow"},
#               None, "write duration (ms)", "number of writers",
#               {"1000_1_1": "1", "1000_2_1": "2", "1000_4_1": "4",
#                "1000_4_2": "8", "1000_4_4": "16", "1000_4_8": "32"}, y_mul=1000)
# plt.savefig("plots/range_vs_writers_by_row.png")
# plt.clf()
#
# catargory_dur({"write by row": "RangeByRow", "write entire file": "RangeWholeFile"},
#               "write pattern", "write duration (ms)", "row length (bytes)",
#               {"1000_2_3": "1000", "10000_2_3": "10,000", "100000_2_3": "100,000",
#                "1000000_2_3": "1,000,000"},
#               violinplot={"bw": 0.01, "linewidth": 0.3, "inner": None,
#                           "scale": "width", "scale_hue": False}, y_mul=1000)
# plt.savefig("plots/range_vs_row_len.png")
# plt.clf()
#
# catargory_dur({"batch": "LsBatch", "stride": "LsStride"},
#               "access pattern", "ls duration (ms)", "number of ministries",
#               {"1": "1", "2": "2", "3": "3", "4": "4", "5": "5"}, y_mul=1000,
#               violinplot = {"bw": 0.01, "linewidth": 0.3, "inner": None,
#                           "scale": "width", "scale_hue": False})
# plt.legend(loc="upper left")
# plt.savefig("plots/ls_vs_numb_ministries.png")
# plt.clf()
#
# dur_dist("LsStride", "ls", "proportion ls requests done within", "time (ms)",
#          xlim=(0, 0.002))
# plt.savefig("plots/ls_stride.svg")
# plt.clf()
#
# dur_dist("Touch", "create", "proportion of touches done within", "time (ms)")
# plt.savefig("plots/touch.svg")
# plt.clf()
#
# dur_vs_time("Touch")
# plt.savefig("plots/touch_vs_time.svg")
# plt.clf()

